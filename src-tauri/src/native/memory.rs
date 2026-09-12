//! SQLite 长期记忆库。
//!
//! 设计：
//! - 独立数据库文件放在该 Agent 的 `config/` 目录下，与权限白名单、
//!   会话记录互不干扰；
//! - `kind` 区分记忆类型（fact / preference / event / reflection），
//!   `tags` 为逗号分隔标签，`importance`（0.0-1.0）参与召回排序；
//! - 检索使用轻量 LIKE 分词匹配：Agent 的记忆量级（数千条以内）下
//!   足够快，且避免引入 FTS5 的编译/版本不确定性；
//! - 所有写入即时提交（Worker 单进程独占该库，无并发竞争）。

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 一条记忆。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// 记忆库统计。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub total: i64,
    pub by_kind: Vec<(String, i64)>,
}

/// SQLite 记忆库。
pub struct MemoryStore {
    conn: Connection,
}

/// 合法记忆类型（防脏数据；其余调用方报错）。
pub const MEMORY_KINDS: &[&str] = &["fact", "preference", "event", "reflection"];

impl MemoryStore {
    /// 打开（必要时创建）记忆库。
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .map_err(|e| AppError::Other(format!("打开记忆库失败: {e}")))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// 在内存中打开（测试与临时会话用）。
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AppError::Other(format!("创建内存记忆库失败: {e}")))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> AppResult<()> {
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS memories (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind        TEXT NOT NULL,
                    content     TEXT NOT NULL,
                    tags        TEXT NOT NULL DEFAULT '',
                    importance  REAL NOT NULL DEFAULT 0.5,
                    created_at  TEXT NOT NULL,
                    updated_at  TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_memories_importance
                    ON memories (importance DESC);
                CREATE INDEX IF NOT EXISTS idx_memories_kind
                    ON memories (kind);
                "#,
            )
            .map_err(|e| AppError::Other(format!("初始化记忆库表结构失败: {e}")))?;
        Ok(())
    }

    fn now() -> String {
        // chrono 已在依赖树中；UTC RFC3339，避免本地时区漂移
        chrono::Utc::now().to_rfc3339()
    }

    fn validate_kind(kind: &str) -> AppResult<()> {
        if !MEMORY_KINDS.contains(&kind) {
            return Err(AppError::Other(format!(
                "非法记忆类型 {kind}，允许: {}",
                MEMORY_KINDS.join(", ")
            )));
        }
        Ok(())
    }

    fn clamp_importance(value: f64) -> f64 {
        value.clamp(0.0, 1.0)
    }

    /// 写入一条记忆，返回其 id。
    pub fn remember(
        &self,
        kind: &str,
        content: &str,
        tags: &[&str],
        importance: f64,
    ) -> AppResult<i64> {
        let content = content.trim();
        if content.is_empty() {
            return Err(AppError::Other("记忆内容不能为空".to_string()));
        }
        Self::validate_kind(kind)?;
        let now = Self::now();
        let tags_joined = tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        self.conn
            .execute(
                "INSERT INTO memories (kind, content, tags, importance, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                rusqlite::params![
                    kind,
                    content,
                    tags_joined,
                    Self::clamp_importance(importance),
                    now
                ],
            )
            .map_err(|e| AppError::Other(format!("写入记忆失败: {e}")))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 按关键词召回（多词 OR 匹配 content / tags），importance 优先。
    pub fn recall(&self, query: &str, limit: usize) -> AppResult<Vec<Memory>> {
        let terms: Vec<&str> = query
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .take(8)
            .collect();
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        // 每个词对 content 与 tags 各做一次 LIKE
        let mut clauses = Vec::new();
        let mut params: Vec<String> = Vec::new();
        for term in &terms {
            let like = format!("%{}%", term.replace('%', ""));
            params.push(like.clone());
            params.push(like);
            clauses.push("(content LIKE ? OR tags LIKE ?)".to_string());
        }
        params.push((limit as i64).to_string());
        let sql = format!(
            "SELECT id, kind, content, tags, importance, created_at, updated_at
             FROM memories
             WHERE {}
             ORDER BY importance DESC, updated_at DESC
             LIMIT ?",
            clauses.join(" OR ")
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| AppError::Other(format!("召回查询失败: {e}")))?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(params.iter()),
                |row| {
                    Ok(Memory {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        content: row.get(2)?,
                        tags: split_tags(&row.get::<_, String>(3)?),
                        importance: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .map_err(|e| AppError::Other(format!("召回查询失败: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AppError::Other(format!("读取记忆行失败: {e}")))?);
        }
        Ok(out)
    }

    /// 最近写入的记忆（时间倒序）。
    pub fn recent(&self, limit: usize) -> AppResult<Vec<Memory>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, content, tags, importance, created_at, updated_at
                 FROM memories
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(|e| AppError::Other(format!("最近记忆查询失败: {e}")))?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(Memory {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    content: row.get(2)?,
                    tags: split_tags(&row.get::<_, String>(3)?),
                    importance: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Other(format!("最近记忆查询失败: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AppError::Other(format!("读取记忆行失败: {e}")))?);
        }
        Ok(out)
    }

    /// 删除一条记忆；返回是否存在。
    pub fn forget(&self, id: i64) -> AppResult<bool> {
        let affected = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", [id])
            .map_err(|e| AppError::Other(format!("删除记忆失败: {e}")))?;
        Ok(affected > 0)
    }

    /// 统计信息。
    pub fn stats(&self) -> AppResult<MemoryStats> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
            .map_err(|e| AppError::Other(format!("统计记忆失败: {e}")))?;
        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM memories GROUP BY kind ORDER BY kind")
            .map_err(|e| AppError::Other(format!("统计记忆失败: {e}")))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| AppError::Other(format!("统计记忆失败: {e}")))?;
        let mut by_kind = Vec::new();
        for row in rows {
            by_kind.push(row.map_err(|e| AppError::Other(format!("统计记忆失败: {e}")))?);
        }
        Ok(MemoryStats { total, by_kind })
    }
}

fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_schema_and_is_reusable() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("memory.db");
        {
            let store = MemoryStore::open(&db).unwrap();
            store.remember("fact", "用户偏好浅色主题", &["ui"], 0.8).unwrap();
        }
        // 重新打开：数据持久
        let store = MemoryStore::open(&db).unwrap();
        let got = store.recall("主题", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "用户偏好浅色主题");
    }

    #[test]
    fn remember_rejects_empty_content_and_bad_kind() {
        let store = MemoryStore::open_in_memory().unwrap();
        assert!(store.remember("fact", "   ", &[], 0.5).is_err());
        assert!(store.remember("diary", "内容", &[], 0.5).is_err());
        assert!(store.remember("fact", "合法", &["t"], 0.5).is_ok());
    }

    #[test]
    fn importance_is_clamped() {
        let store = MemoryStore::open_in_memory().unwrap();
        store.remember("fact", "a", &[], 5.0).unwrap();
        store.remember("fact", "b", &[], -3.0).unwrap();
        let all = store.recent(10).unwrap();
        assert_eq!(all[0].importance, 1.0);
        assert_eq!(all[1].importance, 0.0);
    }

    #[test]
    fn recall_matches_any_term_and_ranks_by_importance() {
        let store = MemoryStore::open_in_memory().unwrap();
        store.remember("fact", "用户使用 Windows 11", &["os"], 0.3).unwrap();
        store.remember("preference", "喜欢 Rust 和深色主题", &["dev"], 0.9).unwrap();
        store.remember("event", "2026-09-12 重构仓库", &["work"], 0.5).unwrap();

        let got = store.recall("Rust 主题", 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "喜欢 Rust 和深色主题");

        // importance 高者在前
        let got = store.recall("Windows 重构", 10).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[0].importance >= got[1].importance);

        assert_eq!(store.recall("", 5).unwrap().len(), 0);
        assert_eq!(store.recall("rust", 0).unwrap().len(), 0);
    }

    #[test]
    fn forget_removes_only_existing() {
        let store = MemoryStore::open_in_memory().unwrap();
        let id = store.remember("fact", "暂时的事实", &[], 0.1).unwrap();
        assert!(store.forget(id).unwrap());
        assert!(!store.forget(id).unwrap());
        assert_eq!(store.stats().unwrap().total, 0);
    }

    #[test]
    fn stats_groups_by_kind() {
        let store = MemoryStore::open_in_memory().unwrap();
        store.remember("fact", "f1", &[], 0.1).unwrap();
        store.remember("fact", "f2", &[], 0.1).unwrap();
        store.remember("event", "e1", &[], 0.1).unwrap();
        let stats = store.stats().unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(
            stats.by_kind,
            vec![("event".to_string(), 1), ("fact".to_string(), 2)]
        );
    }

    #[test]
    fn tags_roundtrip_and_trim() {
        let store = MemoryStore::open_in_memory().unwrap();
        store
            .remember("fact", "带标签", &[" a ", "", "b,c "], 0.5)
            .unwrap();
        let got = store.recall("带标签", 1).unwrap();
        assert_eq!(got[0].tags, vec!["a", "b,c"]);
    }
}

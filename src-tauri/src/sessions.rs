//! 每 Agent 独立的会话存储。
//!
//! 物理隔离策略：每个会话是 `agents/<agent_id>/sessions/<session_id>.json`
//! 下的一个独立文件，`index.json` 保存会话索引。三个 Agent 的会话
//! 互不可见、互不影响；任何一个 Agent 的会话文件损坏，只需删除该
//! Agent 目录下的文件，其余 Agent 完全不受波及。
//!
//! 选型说明：会话为「写少读多、单用户」场景，JSON 文件 + 原子写
//! 足够可靠且零外部依赖；阶段 4 的长期向量记忆将引入 rusqlite。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::paths::{agent_root, validate_agent_id};

/// 一条会话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub id: String,
    /// user / assistant / tool / system
    pub role: String,
    pub content: String,
    /// 附件或工具调用等扩展数据（透传前端结构）。
    #[serde(default)]
    pub meta: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 一个会话。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub messages: Vec<SessionMessage>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 会话索引项（不含消息体，列表时轻量返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub agent_id: String,
    pub message_count: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// 会话存储：按 agent 隔离的根目录 + 内存索引缓存。
pub struct SessionStore {
    base_dir: PathBuf,
    /// agent_id -> (session_id -> Session) 的内存缓存。
    cache: Mutex<HashMap<String, HashMap<String, Session>>>,
}

impl SessionStore {
    pub fn new(base_dir: PathBuf) -> Self {
        let store = Self {
            base_dir,
            cache: Mutex::new(HashMap::new()),
        };
        // 预热：把已有会话读进内存；单个 Agent 的索引损坏只影响它自己
        for agent in crate::paths::AGENT_IDS {
            match store.load_all(agent) {
                Ok(map) => {
                    store.cache.lock().unwrap().insert(agent.to_string(), map);
                }
                Err(e) => {
                    tracing::warn!(agent, error = %e, "加载会话索引失败，按空处理");
                    store
                        .cache
                        .lock()
                        .unwrap()
                        .insert(agent.to_string(), HashMap::new());
                }
            }
        }
        store
    }

    fn sessions_dir(&self, agent_id: &str) -> PathBuf {
        agent_root(&self.base_dir, agent_id).join("sessions")
    }

    /// 单个会话文件的磁盘路径。
    fn session_file(&self, agent_id: &str, session_id: &str) -> PathBuf {
        // session_id 由本模块生成的 UUID，或校验后透传；双重保险防穿越
        let safe: String = session_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.sessions_dir(agent_id).join(format!("{safe}.json"))
    }

    /// 从磁盘加载某 Agent 的全部会话。
    fn load_all(&self, agent_id: &str) -> AppResult<HashMap<String, Session>> {
        validate_agent_id(agent_id)?;
        let dir = self.sessions_dir(agent_id);
        let mut map = HashMap::new();
        if !dir.exists() {
            return Ok(map);
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue; // 跳过 index.json.tmp 等临时文件
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("index.json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .map_err(AppError::Io)
                .and_then(|raw| serde_json::from_str::<Session>(&raw).map_err(AppError::Config))
            {
                Ok(session) => {
                    map.insert(session.id.clone(), session);
                }
                Err(e) => {
                    // 单个会话文件损坏：记录日志并跳过，绝不拖垮整个 Agent
                    tracing::warn!(agent = agent_id, file = %path.display(), error = %e, "会话文件损坏，已跳过");
                }
            }
        }
        Ok(map)
    }

    /// 原子写单个会话文件。
    fn save_session(&self, agent_id: &str, session: &Session) -> AppResult<()> {
        let path = self.session_file(agent_id, &session.id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(session)?)?;
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 列出某 Agent 的会话摘要（按更新时间倒序）。
    pub fn list(&self, agent_id: &str) -> AppResult<Vec<SessionSummary>> {
        validate_agent_id(agent_id)?;
        let guard = self.cache.lock().unwrap();
        let mut list: Vec<SessionSummary> = guard
            .get(agent_id)
            .map(|m| {
                m.values()
                    .map(|s| SessionSummary {
                        id: s.id.clone(),
                        title: s.title.clone(),
                        agent_id: s.agent_id.clone(),
                        message_count: s.messages.len(),
                        created_at: s.created_at,
                        updated_at: s.updated_at,
                    })
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(list)
    }

    /// 读取单个会话（含全部消息）。
    pub fn get(&self, agent_id: &str, session_id: &str) -> AppResult<Session> {
        validate_agent_id(agent_id)?;
        self.cache
            .lock()
            .unwrap()
            .get(agent_id)
            .and_then(|m| m.get(session_id))
            .cloned()
            .ok_or_else(|| AppError::Other(format!("会话不存在: {session_id}")))
    }

    /// 新建会话。
    pub fn create(&self, agent_id: &str, title: &str) -> AppResult<Session> {
        validate_agent_id(agent_id)?;
        let now = chrono::Utc::now();
        let session = Session {
            id: Uuid::new_v4().to_string(),
            title: if title.trim().is_empty() {
                "新会话".to_string()
            } else {
                title.trim().to_string()
            },
            agent_id: agent_id.to_string(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        self.save_session(agent_id, &session)?;
        self.cache
            .lock()
            .unwrap()
            .entry(agent_id.to_string())
            .or_default()
            .insert(session.id.clone(), session.clone());
        tracing::info!(agent = agent_id, session = %session.id, "新建会话");
        Ok(session)
    }

    /// 追加一条消息。
    pub fn append_message(
        &self,
        agent_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        meta: serde_json::Value,
    ) -> AppResult<SessionMessage> {
        validate_agent_id(agent_id)?;
        let msg = SessionMessage {
            id: Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            meta,
            created_at: chrono::Utc::now(),
        };
        let mut guard = self.cache.lock().unwrap();
        let map = guard
            .get_mut(agent_id)
            .ok_or_else(|| AppError::Other(format!("会话不存在: {session_id}")))?;
        let session = map
            .get_mut(session_id)
            .ok_or_else(|| AppError::Other(format!("会话不存在: {session_id}")))?;
        session.messages.push(msg.clone());
        session.updated_at = chrono::Utc::now();
        self.save_session(agent_id, session)?;
        Ok(msg)
    }

    /// 更新会话标题。
    pub fn rename(&self, agent_id: &str, session_id: &str, title: &str) -> AppResult<()> {
        validate_agent_id(agent_id)?;
        let mut guard = self.cache.lock().unwrap();
        let session = guard
            .get_mut(agent_id)
            .and_then(|m| m.get_mut(session_id))
            .ok_or_else(|| AppError::Other(format!("会话不存在: {session_id}")))?;
        session.title = title.trim().to_string();
        session.updated_at = chrono::Utc::now();
        self.save_session(agent_id, session)
    }

    /// 删除会话（磁盘 + 缓存）。
    pub fn delete(&self, agent_id: &str, session_id: &str) -> AppResult<()> {
        validate_agent_id(agent_id)?;
        let path = self.session_file(agent_id, session_id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        if let Some(m) = self.cache.lock().unwrap().get_mut(agent_id) {
            m.remove(session_id);
        }
        tracing::info!(agent = agent_id, session = session_id, "删除会话");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tmp: &std::path::Path) -> SessionStore {
        SessionStore::new(tmp.to_path_buf())
    }

    #[test]
    fn create_append_get_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());

        let session = s.create("deepseek-harness", "测试会话").unwrap();
        s.append_message("deepseek-harness", &session.id, "user", "你好", serde_json::json!({}))
            .unwrap();
        s.append_message("deepseek-harness", &session.id, "assistant", "你好！有什么可以帮你？", serde_json::json!({}))
            .unwrap();

        let loaded = s.get("deepseek-harness", &session.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[1].content, "你好！有什么可以帮你？");

        let list = s.list("deepseek-harness").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message_count, 2);
    }

    #[test]
    fn sessions_are_isolated_per_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let a = s.create("deepseek-harness", "A 的会话").unwrap();
        let c = s.create("codex", "C 的会话").unwrap();

        let list_a = s.list("deepseek-harness").unwrap();
        let list_c = s.list("codex").unwrap();
        assert_eq!(list_a.len(), 1);
        assert_eq!(list_c.len(), 1);
        assert_ne!(list_a[0].id, list_c[0].id);

        // 跨 Agent 读不到对方的会话
        assert!(s.get("codex", &a.id).is_err());
        assert!(s.get("deepseek-harness", &c.id).is_err());
    }

    #[test]
    fn corrupt_session_file_does_not_break_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("agents").join("deepharness").join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.json"), "{ this is not json").unwrap();

        let s = store(tmp.path());
        // 损坏文件被跳过，Agent 的会话列表为空但不报错
        assert_eq!(s.list("deepharness").unwrap().len(), 0);

        // 且新建会话功能不受影响
        let created = s.create("deepharness", "ok").unwrap();
        assert_eq!(s.list("deepharness").unwrap().len(), 1);
        let _ = created;
    }

    #[test]
    fn rename_and_delete_work() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let session = s.create("codex", "old title").unwrap();

        s.rename("codex", &session.id, "new title").unwrap();
        assert_eq!(s.get("codex", &session.id).unwrap().title, "new title");

        s.delete("codex", &session.id).unwrap();
        assert!(s.get("codex", &session.id).is_err());
        assert_eq!(s.list("codex").unwrap().len(), 0);
    }

    #[test]
    fn persist_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let s = store(tmp.path());
            let session = s.create("deepharness", "持久化").unwrap();
            s.append_message("deepharness", &session.id, "user", "消息", serde_json::json!({}))
                .unwrap();
        }
        let s2 = store(tmp.path());
        let list = s2.list("deepharness").unwrap();
        assert_eq!(list.len(), 1);
        let loaded = s2.get("deepharness", &list[0].id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "消息");
    }

    #[test]
    fn cross_agent_access_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        assert!(s.create("evil-agent", "x").is_err());
    }
}

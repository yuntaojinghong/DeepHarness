//! 每 Agent 独立的文件访问权限白名单。
//!
//! 设计要点：
//! - 每个 Agent 拥有独立的 `permissions.json`，互不共享；
//!   Agent A 的授权绝不会自动传递给 Agent B。
//! - Agent 自己的 `workspace/` 目录隐式拥有读写权限，无需显式授权。
//! - 所有路径比较都基于规范化的绝对路径（`dunce::canonicalize`，
//!   避免 Windows `\\?\` 前缀差异），目录授权采用“前缀 + 路径分隔符”
//!   匹配，杜绝 `C:\dir` 授权被 `C:\dir-evil` 绕过。
//! - Windows 文件系统大小写不敏感，路径比较不区分大小写。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use dunce::canonicalize;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::{agent_root, validate_agent_id, AgentDirs};

/// 单条授权的访问模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessMode {
    /// 只读。
    #[serde(rename = "read")]
    Read,
    /// 读写。
    #[serde(rename = "read_write")]
    ReadWrite,
}

impl AccessMode {
    /// 当前模式是否覆盖所需的写权限。
    pub fn covers(&self, need_write: bool) -> bool {
        !need_write || *self == AccessMode::ReadWrite
    }
}

/// 一条授权记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    /// 授权的目录或文件（规范化绝对路径）。
    pub path: PathBuf,
    /// 访问模式。
    pub mode: AccessMode,
    /// 授权时间（UTC）。
    pub granted_at: DateTime<Utc>,
    /// 展示用途的来源说明（例如“用户手动选择”）。
    #[serde(default)]
    pub label: Option<String>,
}

/// 权限存储：内存中持有全部 Agent 的白名单，落盘到各自独立的 JSON 文件。
pub struct PermissionStore {
    /// 应用数据根目录（agents/ 的父目录）。
    base_dir: PathBuf,
    /// agent_id -> 授权列表。
    grants: Mutex<HashMap<String, Vec<Grant>>>,
}

impl PermissionStore {
    /// 创建存储并尝试加载磁盘上的既有授权；加载失败按空处理（不 panic）。
    pub fn new(base_dir: PathBuf) -> Self {
        let store = Self {
            base_dir,
            grants: Mutex::new(HashMap::new()),
        };
        for agent in crate::paths::AGENT_IDS {
            match store.load_agent(agent) {
                Ok(list) => {
                    store.grants.lock().unwrap().insert(agent.to_string(), list);
                }
                Err(e) => {
                    tracing::warn!(agent, error = %e, "加载权限白名单失败，按空白名单处理");
                    store.grants.lock().unwrap().insert(agent.to_string(), Vec::new());
                }
            }
        }
        store
    }

    /// 某个 Agent 的 permissions.json 路径。
    fn grant_file(&self, agent_id: &str) -> PathBuf {
        agent_root(&self.base_dir, agent_id).join("permissions.json")
    }

    /// 从磁盘读取某 Agent 的授权列表；文件不存在视为空。
    fn load_agent(&self, agent_id: &str) -> AppResult<Vec<Grant>> {
        let path = self.grant_file(agent_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)?;
        let grants = serde_json::from_str(&raw)?;
        Ok(grants)
    }

    /// 将某 Agent 的授权列表写回磁盘（原子写：先写临时文件再重命名）。
    fn save_agent(&self, agent_id: &str, grants: &[Grant]) -> AppResult<()> {
        let path = self.grant_file(agent_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(grants)?)?;
        // Windows 上 rename 到已存在目标会失败，先移除旧文件
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn snapshot(&self, agent_id: &str) -> Vec<Grant> {
        self.grants
            .lock()
            .unwrap()
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 列出某 Agent 的全部授权。
    pub fn list(&self, agent_id: &str) -> AppResult<Vec<Grant>> {
        validate_agent_id(agent_id)?;
        Ok(self.snapshot(agent_id))
    }

    /// 判断某 Agent 对给定路径是否具备所需权限（含隐式 workspace）。
    pub fn check(&self, agent_id: &str, dirs: &AgentDirs, path: &Path, need_write: bool) -> bool {
        if validate_agent_id(agent_id).is_err() {
            return false;
        }
        let Some(req) = canonicalize_lenient(path) else {
            return false;
        };
        let req_str = path_key(&req);

        // 1) 隐式授权：Agent 自己的 workspace。
        //    workspace 同样要先规范化，否则 Windows 上会因 \\?\ 前缀 /
        //    短路径名 / 大小写差异误判（测试真实复现过该缺陷）。
        let workspace_key = canonicalize_lenient(&dirs.workspace)
            .map(|p| path_key(&p))
            .unwrap_or_else(|| path_key(&dirs.workspace));
        if path_within(&req_str, &workspace_key) {
            return true;
        }

        // 2) 显式授权列表
        for grant in self.snapshot(agent_id) {
            if !grant.mode.covers(need_write) {
                continue;
            }
            if path_within(&req_str, &path_key(&grant.path)) {
                return true;
            }
        }
        false
    }

    /// 授予某 Agent 对路径的访问权；若已有覆盖性授权则原样返回。
    pub fn grant(
        &self,
        agent_id: &str,
        path: &Path,
        mode: AccessMode,
        label: Option<&str>,
    ) -> AppResult<Grant> {
        validate_agent_id(agent_id)?;
        let canonical = canonicalize_lenient(path)
            .ok_or_else(|| AppError::PathNotFound(path.display().to_string()))?;

        let mut all = self.grants.lock().unwrap();
        let list = all.entry(agent_id.to_string()).or_default();

        // 已有完全相同路径的授权：按需升级为读写
        if let Some(existing) = list
            .iter_mut()
            .find(|g| path_key(&g.path) == path_key(&canonical))
        {
            if existing.mode != mode {
                existing.mode = mode;
                existing.granted_at = Utc::now();
                let updated = existing.clone();
                self.save_agent(agent_id, list)?;
                tracing::info!(agent = agent_id, path = %canonical.display(), "权限升级为读写");
                return Ok(updated);
            }
            return Ok(existing.clone());
        }

        let grant = Grant {
            path: canonical,
            mode,
            granted_at: Utc::now(),
            label: label.map(|s| s.to_string()),
        };
        list.push(grant.clone());
        self.save_agent(agent_id, list)?;
        tracing::info!(agent = agent_id, path = %grant.path.display(), mode = ?grant.mode, "新增授权");
        Ok(grant)
    }

    /// 撤销某 Agent 对给定路径的授权（精确匹配）。
    pub fn revoke(&self, agent_id: &str, path: &Path) -> AppResult<()> {
        validate_agent_id(agent_id)?;
        let target = canonicalize_lenient(path)
            .map(|p| path_key(&p))
            .unwrap_or_else(|| path_key(path));

        let mut all = self.grants.lock().unwrap();
        let list = all.entry(agent_id.to_string()).or_default();
        let before = list.len();
        list.retain(|g| path_key(&g.path) != target);
        let removed = before - list.len();
        self.save_agent(agent_id, list)?;
        tracing::info!(agent = agent_id, path = %path.display(), removed, "撤销授权");
        Ok(())
    }
}

/// 规范化路径（容忍目标尚不存在）：对最深存在祖先做 canonicalize，
/// 再拼回剩余部分。这是“写新文件”场景的关键。
pub fn canonicalize_lenient(path: &Path) -> Option<PathBuf> {
    if let Ok(p) = canonicalize(path) {
        return Some(p);
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    // 逐级向上找第一个存在的祖先
    let mut ancestors = abs.ancestors().skip(1);
    while let Some(anc) = ancestors.next() {
        if let Ok(c) = canonicalize(anc) {
            let remainder = abs.strip_prefix(anc).ok()?;
            return Some(c.join(remainder));
        }
    }
    None
}

/// 生成用于比较的路径键：小写化（Windows 大小写不敏感）、统一分隔符。
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_ascii_lowercase()
}

/// 判断 `candidate` 是否等于 `root` 或位于 `root` 内部。
/// 使用“前缀 + 分隔符”匹配，避免 `C:\dir` 匹配到 `C:\dir-evil`。
fn path_within(candidate: &str, root: &str) -> bool {
    if candidate == root {
        return true;
    }
    let root_with_sep = if root.ends_with('\\') {
        root.to_string()
    } else {
        format!("{root}\\")
    };
    candidate.starts_with(&root_with_sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tmp: &Path) -> PermissionStore {
        PermissionStore::new(tmp.to_path_buf())
    }

    fn make_dirs(tmp: &Path, agent: &str) -> AgentDirs {
        AgentDirs::from_root(agent_root(tmp, agent), agent)
    }

    #[test]
    fn workspace_is_implicitly_accessible() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let dirs = make_dirs(tmp.path(), "deepseek-harness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let f = dirs.workspace.join("a.txt");
        std::fs::write(&f, "hi").unwrap();
        assert!(s.check("deepseek-harness", &dirs, &f, false));
        assert!(s.check("deepseek-harness", &dirs, &f, true));
    }

    #[test]
    fn workspace_of_other_agent_is_denied() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let dirs_a = make_dirs(tmp.path(), "deepseek-harness");
        let dirs_c = make_dirs(tmp.path(), "codex");
        std::fs::create_dir_all(&dirs_c.workspace).unwrap();
        let f = dirs_c.workspace.join("a.txt");
        std::fs::write(&f, "hi").unwrap();
        // Agent A 不能访问 Agent C 的 workspace
        assert!(!s.check("deepseek-harness", &dirs_a, &f, false));
    }

    #[test]
    fn outside_paths_denied_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let dirs = make_dirs(tmp.path(), "deepharness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let secret = tempfile::tempdir().unwrap();
        let f = secret.path().join("x.txt");
        std::fs::write(&f, "s").unwrap();
        assert!(!s.check("deepharness", &dirs, &f, false));
    }

    #[test]
    fn grant_then_check_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let dirs = make_dirs(tmp.path(), "codex");
        std::fs::create_dir_all(&dirs.workspace).unwrap();

        let external = tempfile::tempdir().unwrap();
        let f = external.path().join("data.txt");
        std::fs::write(&f, "d").unwrap();

        // 只读授权：读可以，写不行
        s.grant("codex", external.path(), AccessMode::Read, Some("测试"))
            .unwrap();
        assert!(s.check("codex", &dirs, &f, false));
        assert!(!s.check("codex", &dirs, &f, true));

        // 升级为读写
        s.grant("codex", external.path(), AccessMode::ReadWrite, Some("测试"))
            .unwrap();
        assert!(s.check("codex", &dirs, &f, true));

        // 撤销后重新拒绝
        s.revoke("codex", external.path()).unwrap();
        assert!(!s.check("codex", &dirs, &f, false));
    }

    #[test]
    fn grants_are_isolated_between_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let dirs_a = make_dirs(tmp.path(), "deepseek-harness");
        std::fs::create_dir_all(&dirs_a.workspace).unwrap();

        let external = tempfile::tempdir().unwrap();
        s.grant("deepseek-harness", external.path(), AccessMode::ReadWrite, None)
            .unwrap();

        let dirs_c = make_dirs(tmp.path(), "codex");
        std::fs::create_dir_all(&dirs_c.workspace).unwrap();

        // Agent A 拿到的授权不能传导给 Agent C
        assert!(s.check("deepseek-harness", &dirs_a, external.path(), true));
        assert!(!s.check("codex", &dirs_c, external.path(), true));
    }

    #[test]
    fn grants_persist_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let s = store(tmp.path());
            s.grant("deepharness", tmp.path(), AccessMode::Read, None).unwrap();
        }
        let s2 = store(tmp.path());
        let list = s2.list("deepharness").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].mode, AccessMode::Read);
    }

    #[test]
    fn prefix_match_does_not_leak_to_sibling_dirs() {
        // 授权 C:\dir 不应让 C:\dir-evil 通过
        assert!(path_within("c:\\dir\\file", "c:\\dir"));
        assert!(!path_within("c:\\dir-evil\\file", "c:\\dir"));
        assert!(path_within("c:\\dir", "c:\\dir"));
        assert!(!path_within("c:\\directory", "c:\\dir"));
    }

    #[test]
    fn canonicalize_lenient_handles_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("new_sub_dir").join("new_file.txt");
        let resolved = canonicalize_lenient(&missing).unwrap();
        assert!(resolved.ends_with("new_sub_dir\\new_file.txt") || resolved.ends_with("new_sub_dir/new_file.txt"));
        assert!(resolved.starts_with(canonicalize(tmp.path()).unwrap()));
    }
}

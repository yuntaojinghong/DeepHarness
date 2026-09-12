//! Agent 目录布局与身份校验。
//!
//! DeepHarness 中的三个 Agent 相互完全隔离：
//!
//! ```text
//! <app_data_dir>/
//!   agents/
//!     deepseek-harness/
//!       workspace/    # 该 Agent 默认唯一可读写的目录
//!       config/       # 独立配置
//!       sessions/     # 独立会话数据库
//!       logs/         # 独立日志
//!       plugins/      # 独立插件作用域
//!       permissions.json  # 独立权限白名单
//!     codex/…
//!     deepharness/…
//! ```

use std::path::PathBuf;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 全部合法的 Agent 标识。新增 Agent 时在此注册。
pub const AGENT_IDS: [&str; 3] = ["deepseek-harness", "codex", "deepharness"];

/// 某个 Agent 的完整目录布局。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDirs {
    /// Agent 标识。
    pub agent_id: String,
    /// Agent 隔离根目录（agents/<agent_id>/）。
    pub root: PathBuf,
    /// 工作区目录：Agent 默认唯一可自由读写的位置。
    pub workspace: PathBuf,
    /// 配置目录。
    pub config: PathBuf,
    /// 会话数据目录。
    pub sessions: PathBuf,
    /// 日志目录。
    pub logs: PathBuf,
    /// 插件目录。
    pub plugins: PathBuf,
}

impl AgentDirs {
    /// 从隔离根目录推导出完整布局（不做任何磁盘创建）。
    pub fn from_root(root: PathBuf, agent_id: &str) -> Self {
        Self {
            root: root.clone(),
            workspace: root.join("workspace"),
            config: root.join("config"),
            sessions: root.join("sessions"),
            logs: root.join("logs"),
            plugins: root.join("plugins"),
            agent_id: agent_id.to_string(),
        }
    }
}

/// 校验 Agent 标识是否合法。
///
/// 非法标识（不在注册表中、含路径分隔符或 ".."）一律拒绝，
/// 防止通过伪造 agent 参数进行路径穿越。
pub fn validate_agent_id(agent_id: &str) -> AppResult<()> {
    if !AGENT_IDS.contains(&agent_id) {
        return Err(AppError::UnknownAgent(agent_id.to_string()));
    }
    if agent_id.contains('\\')
        || agent_id.contains('/')
        || agent_id.contains("..")
        || agent_id.contains(':')
    {
        // 双保险：即便未来注册表出现疏漏，含路径字符的 id 也不会通过
        return Err(AppError::UnknownAgent(agent_id.to_string()));
    }
    Ok(())
}

/// 计算某个 Agent 的隔离根目录：`<base>/agents/<agent_id>/`。
pub fn agent_root(base: &std::path::Path, agent_id: &str) -> PathBuf {
    base.join("agents").join(agent_id)
}

/// 确保某个 Agent 的全部目录存在（幂等），并返回目录布局。
pub fn ensure_agent_dirs(base: &std::path::Path, agent_id: &str) -> AppResult<AgentDirs> {
    validate_agent_id(agent_id)?;
    let dirs = AgentDirs::from_root(agent_root(base, agent_id), agent_id);
    std::fs::create_dir_all(&dirs.workspace)?;
    std::fs::create_dir_all(&dirs.config)?;
    std::fs::create_dir_all(&dirs.sessions)?;
    std::fs::create_dir_all(&dirs.logs)?;
    std::fs::create_dir_all(&dirs.plugins)?;
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_agents_validate() {
        for id in AGENT_IDS {
            assert!(validate_agent_id(id).is_ok(), "{id} 应当合法");
        }
    }

    #[test]
    fn unknown_agent_rejected() {
        assert!(validate_agent_id("hacker").is_err());
        assert!(validate_agent_id("").is_err());
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(validate_agent_id("../evil").is_err());
        assert!(validate_agent_id("deepseek-harness/../codex").is_err());
        assert!(validate_agent_id("C:\\evil").is_err());
    }

    #[test]
    fn agent_dirs_layout_is_complete() {
        let dirs = AgentDirs::from_root(std::path::PathBuf::from("X:/base/agents/codex"), "codex");
        assert!(dirs.workspace.starts_with(&dirs.root));
        assert!(dirs.config.starts_with(&dirs.root));
        assert!(dirs.sessions.starts_with(&dirs.root));
        assert!(dirs.logs.starts_with(&dirs.root));
        assert!(dirs.plugins.starts_with(&dirs.root));
        assert!(dirs.workspace.ends_with("workspace"));
    }

    #[test]
    fn ensure_agent_dirs_creates_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ensure_agent_dirs(tmp.path(), "deepharness").unwrap();
        assert!(dirs.workspace.is_dir());
        assert!(dirs.config.is_dir());
        assert!(dirs.sessions.is_dir());
        assert!(dirs.logs.is_dir());
        assert!(dirs.plugins.is_dir());
    }

    #[test]
    fn ensure_agent_dirs_rejects_unknown_agent() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(ensure_agent_dirs(tmp.path(), "nope").is_err());
    }
}

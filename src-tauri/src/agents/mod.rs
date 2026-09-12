//! Agent 运行时抽象层。
//!
//! 三个 Agent（DeepSeek Harness / Codex / DeepHarness）各自实现
//! `AgentRuntime`，拥有独立的子进程、配置目录、会话库、日志与
//! 资源配额（Windows Job Object）。任何一个运行时的崩溃 / 卡死
//! / 重启都不会波及其余两个。

pub mod agent_config;
pub mod codex;
pub mod dsh;
pub mod native;
pub mod registry;
pub mod worker;

use serde::Serialize;

use crate::error::AppResult;
use crate::paths::AgentDirs;

/// Agent 运行状态。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum AgentStatus {
    /// 已停止。
    Stopped,
    /// 启动中。
    Starting,
    /// 运行中。
    Running,
    /// 已崩溃 / 异常退出（附带最后一次的错误摘要）。
    Crashed { reason: String },
}

impl AgentStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, AgentStatus::Running)
    }
}

/// 单个 Agent 运行时的抽象接口。
///
/// 实现约定：
/// - `start` 必须幂等（重复调用先停后启）；
/// - `stop` 必须幂等且不 panic；
/// - 崩溃检测：`status()` 通过 `try_wait` 观察子进程，退出即报
///   `Crashed`，由注册表广播给前端；
/// - 实现内部不得 panic —— 所有失败路径都返回 `AppError`。
pub trait AgentRuntime: Send + Sync {
    /// Agent 标识（与 paths::AGENT_IDS 一致）。
    fn id(&self) -> &'static str;

    /// 展示名称。
    fn display_name(&self) -> &'static str;

    /// 启动运行时。
    fn start(&self) -> AppResult<()>;

    /// 停止运行时（幂等）。
    fn stop(&self) -> AppResult<()>;

    /// 当前状态（内部做崩溃检测）。
    fn status(&self) -> AgentStatus;

    /// 该 Agent 的隔离目录布局。
    fn dirs(&self) -> &AgentDirs;

    /// 该 Agent 官方 Web UI 的**可直接打开**地址。
    ///
    /// 返回 `None` 表示没有独立 Web UI，或当前尚未就绪。
    /// 两点实现约定：
    /// - 地址里必须带上必要的鉴权参数（例如 dsh 0.1.5 起每次启动都会
    ///   重新生成的 token），否则打开的是一个 401 页面；
    /// - 该地址不一定在进程启动瞬间就已可用，实现可以做**有界等待**，
    ///   但必须有明确上限，且未运行时要立即返回 `None`。
    fn web_ui_url(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_with_tag() {
        let s = serde_json::to_string(&AgentStatus::Running).unwrap();
        assert_eq!(s, r#"{"state":"running"}"#);
        let c = serde_json::to_string(&AgentStatus::Crashed {
            reason: "exit code 1".into(),
        })
        .unwrap();
        assert!(c.contains("crashed"));
        assert!(c.contains("exit code 1"));
    }

    #[test]
    fn running_check() {
        assert!(AgentStatus::Running.is_running());
        assert!(!AgentStatus::Stopped.is_running());
        assert!(!AgentStatus::Starting.is_running());
    }
}

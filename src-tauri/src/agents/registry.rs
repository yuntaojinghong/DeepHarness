//! Agent 注册表：统一持有三个运行时，提供状态查询 / 启停 / 崩溃隔离。
//!
//! 注册表是三个 Agent 的唯一交汇点：除了在注册时各自独立创建之外，
//! 运行时之间没有任何共享状态；任何一个运行时的崩溃只会反映在它
//! 自己的状态上。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::paths::{agent_root, ensure_agent_dirs};

/// Agent 状态的对外视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOverview {
    pub id: String,
    pub display_name: String,
    pub status: crate::agents::AgentStatus,
}

/// 注册表。
pub struct AgentRegistry {
    runtimes: Mutex<HashMap<&'static str, Box<dyn crate::agents::AgentRuntime>>>,
}

impl AgentRegistry {
    /// 注册一个运行时（每个 id 只能注册一次）。
    pub fn register(&self, runtime: Box<dyn crate::agents::AgentRuntime>) -> AppResult<()> {
        let mut map = self.runtimes.lock().unwrap();
        if map.contains_key(runtime.id()) {
            return Err(AppError::Other(format!(
                "Agent {} 已注册",
                runtime.id()
            )));
        }
        map.insert(runtime.id(), runtime);
        Ok(())
    }

    /// 列出全部 Agent 及状态。
    pub fn overview(&self) -> Vec<AgentOverview> {
        let map = self.runtimes.lock().unwrap();
        let mut list: Vec<AgentOverview> = map
            .values()
            .map(|rt| AgentOverview {
                id: rt.id().to_string(),
                display_name: rt.display_name().to_string(),
                status: rt.status(),
            })
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    fn get(&self, agent: &str) -> AppResult<std::sync::MutexGuard<'_, HashMap<&'static str, Box<dyn crate::agents::AgentRuntime>>>> {
        crate::paths::validate_agent_id(agent)?;
        let map = self.runtimes.lock().unwrap();
        if !map.contains_key(agent) {
            return Err(AppError::UnknownAgent(agent.to_string()));
        }
        Ok(map)
    }

    /// 启动某个 Agent（崩溃隔离：错误只属于该 Agent）。
    pub fn start(&self, agent: &str) -> AppResult<()> {
        let map = self.get(agent)?;
        map.get(agent).unwrap().start()
    }

    /// 停止某个 Agent。
    pub fn stop(&self, agent: &str) -> AppResult<()> {
        let map = self.get(agent)?;
        map.get(agent).unwrap().stop()
    }

    /// 查询某个 Agent 状态。
    pub fn status_of(&self, agent: &str) -> AppResult<crate::agents::AgentStatus> {
        let map = self.get(agent)?;
        Ok(map.get(agent).unwrap().status())
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self {
            runtimes: Mutex::new(HashMap::new()),
        }
    }
}

// ---- Windows Job Object：每 Agent 独立资源配额 ----

/// 为单个 Agent 创建 Job Object。
///
/// - 内存配额 `memory_limit_bytes`：超限时内核只终止该 Job 内的进程；
/// - KILL_ON_JOB_CLOSE：主应用意外退出时，所有 Agent 子进程随之回收；
/// - 非 Windows 平台返回 None（配额由各运行时自行以其他方式约束）。
#[cfg(windows)]
pub fn create_agent_job(
    memory_limit_bytes: usize,
) -> Option<win32job::Job> {
    let job = win32job::Job::create().ok()?;
    let mut info = win32job::ExtendedLimitInfo::new();
    // win32job 2.0.3 未暴露 per-process commit limit（JOB_OBJECT_LIMIT_PROCESS_MEMORY），
    // 这里用 working-set 上下限作为实际内存约束；超限时内核回收该 Job 内进程的物理页。
    info.limit_working_memory(memory_limit_bytes, memory_limit_bytes);
    info.limit_kill_on_job_close();
    job.set_extended_limit_info(&info).ok()?;
    Some(job)
}

#[cfg(not(windows))]
pub fn create_agent_job(_memory_limit_bytes: usize) -> Option<()> {
    None
}

// ---- Tauri 命令 ----

/// 列出全部 Agent 概览。
#[tauri::command]
pub fn agent_list(registry: tauri::State<'_, AgentRegistry>) -> Vec<AgentOverview> {
    registry.overview()
}

/// 启动某个 Agent。
#[tauri::command]
pub fn agent_start(registry: tauri::State<'_, AgentRegistry>, agent: String) -> AppResult<()> {
    registry.start(&agent)
}

/// 停止某个 Agent。
#[tauri::command]
pub fn agent_stop(registry: tauri::State<'_, AgentRegistry>, agent: String) -> AppResult<()> {
    registry.stop(&agent)
}

/// 查询某个 Agent 状态。
#[tauri::command]
pub fn agent_status(
    registry: tauri::State<'_, AgentRegistry>,
    agent: String,
) -> AppResult<crate::agents::AgentStatus> {
    registry.status_of(&agent)
}

// ---- 会话命令（每 Agent 独立会话库） ----

/// 列出某 Agent 的会话摘要。
#[tauri::command]
pub fn session_list(
    agent: String,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<Vec<crate::sessions::SessionSummary>> {
    store.list(&agent)
}

/// 读取某个会话。
#[tauri::command]
pub fn session_get(
    agent: String,
    session_id: String,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<crate::sessions::Session> {
    store.get(&agent, &session_id)
}

/// 新建会话。
#[tauri::command]
pub fn session_create(
    agent: String,
    title: String,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<crate::sessions::Session> {
    store.create(&agent, &title)
}

/// 追加会话消息。
#[tauri::command]
pub fn session_append_message(
    agent: String,
    session_id: String,
    role: String,
    content: String,
    meta: Option<serde_json::Value>,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<crate::sessions::SessionMessage> {
    store.append_message(&agent, &session_id, &role, &content, meta.unwrap_or(serde_json::Value::Null))
}

/// 重命名会话。
#[tauri::command]
pub fn session_rename(
    agent: String,
    session_id: String,
    title: String,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<()> {
    store.rename(&agent, &session_id, &title)
}

/// 删除会话。
#[tauri::command]
pub fn session_delete(
    agent: String,
    session_id: String,
    store: tauri::State<'_, crate::sessions::SessionStore>,
) -> AppResult<()> {
    store.delete(&agent, &session_id)
}

/// 确保三个 Agent 的目录树在启动时就存在。
pub fn ensure_all_agent_dirs(base: &std::path::Path) -> AppResult<()> {
    for id in crate::paths::AGENT_IDS {
        ensure_agent_dirs(base, id)?;
        let _ = agent_root(base, id); // 布局校验
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentStatus;
    use crate::paths::AgentDirs;

    struct FakeRuntime {
        id: &'static str,
        name: &'static str,
        status: Mutex<AgentStatus>,
    }

    impl FakeRuntime {
        fn new(id: &'static str) -> Self {
            Self {
                id,
                name: id,
                status: Mutex::new(AgentStatus::Stopped),
            }
        }
    }

    impl crate::agents::AgentRuntime for FakeRuntime {
        fn id(&self) -> &'static str {
            self.id
        }
        fn display_name(&self) -> &'static str {
            self.name
        }
        fn start(&self) -> crate::error::AppResult<()> {
            *self.status.lock().unwrap() = AgentStatus::Running;
            Ok(())
        }
        fn stop(&self) -> crate::error::AppResult<()> {
            *self.status.lock().unwrap() = AgentStatus::Stopped;
            Ok(())
        }
        fn status(&self) -> AgentStatus {
            self.status.lock().unwrap().clone()
        }
        fn dirs(&self) -> &AgentDirs {
            // 测试桩：不会真正使用
            unreachable!("FakeRuntime 不持有目录");
        }
    }

    #[test]
    fn register_start_stop_status_flow() {
        let registry = AgentRegistry::default();
        registry.register(Box::new(FakeRuntime::new("deepseek-harness"))).unwrap();
        registry.register(Box::new(FakeRuntime::new("codex"))).unwrap();
        registry.register(Box::new(FakeRuntime::new("deepharness"))).unwrap();

        // 重复注册被拒绝
        assert!(registry
            .register(Box::new(FakeRuntime::new("codex")))
            .is_err());

        assert_eq!(registry.overview().len(), 3);

        // 启停彼此独立
        registry.start("codex").unwrap();
        assert_eq!(
            registry.status_of("codex").unwrap(),
            AgentStatus::Running
        );
        assert_eq!(
            registry.status_of("deepharness").unwrap(),
            AgentStatus::Stopped,
            "另一个 Agent 不应被影响"
        );

        registry.stop("codex").unwrap();
        assert_eq!(registry.status_of("codex").unwrap(), AgentStatus::Stopped);
    }

    #[test]
    fn unknown_agent_rejected() {
        let registry = AgentRegistry::default();
        assert!(registry.start("nope").is_err());
        assert!(registry.status_of("nope").is_err());
    }

    #[test]
    fn ensure_all_creates_three_trees() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_all_agent_dirs(tmp.path()).unwrap();
        for id in crate::paths::AGENT_IDS {
            assert!(agent_root(tmp.path(), id).join("workspace").is_dir());
            assert!(agent_root(tmp.path(), id).join("sessions").is_dir());
        }
    }
}

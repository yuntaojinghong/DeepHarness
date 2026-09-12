//! DeepSeek Harness 运行时：管理内置 dsh CLI 子进程。
//!
//! 设计要点：
//! - 依赖路径（node.exe / dsh bin.js）在构造时解析注入，`start()`
//!   不依赖任何全局状态，可测试；
//! - 子进程加入独立 Job Object（内存配额 + kill-on-close），
//!   应用退出时 dsh 一定被回收，内存超限时只有 dsh 自己被杀；
//! - stdout / stderr 全量写入该 Agent 独立日志
//!   `agents/deepseek-harness/logs/dsh-runtime.log`。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

// lib 构建中 wait_ready 为死代码(仅测试使用), 但测试构建需要此 trait 导入
#[allow(unused_imports)]
use crate::agents::AgentRuntime;
use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;
use crate::permissions::canonicalize_lenient;
use tauri::Manager;

/// dsh 监听端口。
pub const DSH_PORT: u16 = 3080;

/// 就绪等待上限（毫秒）。
pub const READY_TIMEOUT_MS: u64 = 90_000;

/// dsh 进程内存配额：1.5 GB。
const DSH_MEMORY_LIMIT_BYTES: usize = 1_500 * 1024 * 1024;

/// DeepSeek Harness 运行时。
pub struct DshRuntime {
    dirs: AgentDirs,
    node_path: PathBuf,
    dsh_bin_path: PathBuf,
    child: Mutex<Option<Child>>,
    failed_reason: Mutex<Option<String>>,
}

impl DshRuntime {
    /// 创建运行时。
    ///
    /// `node_path` / `dsh_bin_path` 由调用方（lib.rs setup，持有
    /// AppHandle）通过 `find_resource` 解析后传入；解析失败时运行时
    /// 仍可创建，但 `start()` 会返回明确错误。
    pub fn new(dirs: AgentDirs, node_path: Option<PathBuf>, dsh_bin_path: Option<PathBuf>) -> Self {
        Self {
            dirs,
            node_path: node_path.unwrap_or_default(),
            dsh_bin_path: dsh_bin_path.unwrap_or_default(),
            child: Mutex::new(None),
            failed_reason: Mutex::new(None),
        }
    }

    /// 从应用资源中定位文件（lib.rs setup 阶段调用）。
    pub fn find_resource(app: &tauri::AppHandle, relative: &str) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("_up_").join("resources").join(relative));
                candidates.push(dir.join("resources").join(relative));
                candidates.push(dir.join(relative));
            }
        }
        if let Ok(res) = app.path().resource_dir() {
            candidates.push(res.join(relative));
            candidates.push(res.join("resources").join(relative));
            candidates.push(res.join("_up_").join("resources").join(relative));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn kill_stale_port_holder() {
        if let Ok(out) = Command::new("netstat").args(["-ano"]).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            let needle = format!(":{DSH_PORT}");
            for line in s.lines() {
                if line.contains(&needle) && line.to_ascii_uppercase().contains("LISTENING") {
                    if let Some(pid) = line.split_whitespace().last() {
                        let _ = Command::new("taskkill").args(["/F", "/PID", pid]).output();
                    }
                }
            }
        }
    }

    /// 轮询 TCP 端口等待服务就绪。
    fn wait_ready(deadline_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while (start.elapsed().as_millis() as u64) < deadline_ms {
            if port_alive(DSH_PORT) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        false
    }

    /// 端口是否存活（健康检查）。
    pub fn is_endpoint_alive(&self) -> bool {
        port_alive(DSH_PORT)
    }

    /// 读取最近 8KB 运行日志（前端日志面板用）。
    pub fn recent_log(&self) -> String {
        let path = runtime_log_path(&self.dirs);
        const TAIL: u64 = 8 * 1024;
        let Ok(mut f) = File::open(&path) else {
            return String::new();
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let start = len.saturating_sub(TAIL);
        if f.seek(SeekFrom::Start(start)).is_err() {
            return String::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return String::new();
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    fn last_error_snapshot(&self) -> Option<String> {
        self.failed_reason.lock().unwrap().clone()
    }
}

impl crate::agents::AgentRuntime for DshRuntime {
    fn id(&self) -> &'static str {
        "deepseek-harness"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn start(&self) -> AppResult<()> {
        if self.node_path.as_os_str().is_empty() {
            return Err(AppError::Other("内置 Node 缺失".to_string()));
        }
        if self.dsh_bin_path.as_os_str().is_empty() {
            return Err(AppError::Other("dsh 缺失".to_string()));
        }

        // 幂等：先停掉旧进程
        self.stop()?;

        *self.failed_reason.lock().unwrap() = None;
        Self::kill_stale_port_holder();

        let log_path = runtime_log_path(&self.dirs);
        if let Some(p) = log_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let stderr_log = File::create(&log_path)?;
        let stdout_log = stderr_log.try_clone()?;

        let data_dir =
            canonicalize_lenient(&self.dirs.root).unwrap_or_else(|| self.dirs.root.clone());

        let mut child = Command::new(&self.node_path)
            .env("DSH_HOME", &data_dir)
            .arg(&self.dsh_bin_path)
            .arg("web")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(DSH_PORT.to_string())
            .arg("--no-open")
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(stderr_log))
            .spawn()
            .map_err(|e| AppError::Other(format!("启动 dsh 失败: {e}")))?;

        // 加入 Job Object：内存配额 + 应用退出时自动回收
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if let Some(job) = crate::agents::registry::create_agent_job(DSH_MEMORY_LIMIT_BYTES) {
                let _ = job.assign_process(child.as_raw_handle() as isize);
            }
            // Job 句柄 drop 后内核引用计数维持 Job 存活，kill-on-close 生效
        }

        // 等待就绪；期间进程提前退出则带上日志摘要报错
        let mut early_exit: Option<String> = None;
        let start = std::time::Instant::now();
        while (start.elapsed().as_millis() as u64) < READY_TIMEOUT_MS {
            if let Ok(Some(status)) = child.try_wait() {
                early_exit = Some(format!("dsh 提前退出: {status}"));
                break;
            }
            if port_alive(DSH_PORT) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        if let Some(reason) = early_exit {
            let _ = child.kill();
            let _ = child.wait();
            let summary: String = format!("{reason}\n{}", self.recent_log())
                .trim()
                .chars()
                .take(800)
                .collect();
            *self.failed_reason.lock().unwrap() = Some(summary.clone());
            return Err(AppError::Other(summary));
        }

        if !port_alive(DSH_PORT) {
            let _ = child.kill();
            let _ = child.wait();
            *self.failed_reason.lock().unwrap() = Some("dsh 启动超时".to_string());
            return Err(AppError::Other(format!(
                "dsh 启动超时（{} 秒）",
                READY_TIMEOUT_MS / 1000
            )));
        }

        *self.child.lock().unwrap() = Some(child);
        tracing::info!(port = DSH_PORT, "DeepSeek Harness 运行时已启动");
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        let mut guard = self.child.lock().unwrap();
        if let Some(mut c) = guard.take() {
            let _ = c.kill();
            let _ = c.wait();
            tracing::info!("DeepSeek Harness 运行时已停止");
        }
        Ok(())
    }

    fn status(&self) -> crate::agents::AgentStatus {
        let mut guard = self.child.lock().unwrap();
        if let Some(c) = guard.as_mut() {
            match c.try_wait() {
                Ok(Some(status)) => {
                    let reason = format!("dsh 进程退出: {status}");
                    *guard = None;
                    drop(guard);
                    *self.failed_reason.lock().unwrap() = Some(reason.clone());
                    return crate::agents::AgentStatus::Crashed { reason };
                }
                Ok(None) => return crate::agents::AgentStatus::Running,
                Err(e) => {
                    return crate::agents::AgentStatus::Crashed {
                        reason: format!("进程状态不可读: {e}"),
                    };
                }
            }
        }
        if let Some(err) = self.last_error_snapshot() {
            crate::agents::AgentStatus::Crashed { reason: err }
        } else {
            crate::agents::AgentStatus::Stopped
        }
    }

    fn dirs(&self) -> &AgentDirs {
        &self.dirs
    }
}

/// dsh 运行日志路径。
pub fn runtime_log_path(dirs: &AgentDirs) -> PathBuf {
    dirs.logs.join("dsh-runtime.log")
}

/// 端口健康检查。
pub fn port_alive(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_without_paths_starts_nothing() {
        let dirs = AgentDirs::from_root(PathBuf::from("X:/never"), "deepseek-harness");
        let rt = DshRuntime::new(dirs, None, None);
        assert_eq!(rt.status(), crate::agents::AgentStatus::Stopped);
        // start 因缺资源而失败，但不 panic、不留僵尸状态
        assert!(crate::agents::AgentRuntime::start(&rt).is_err());
        assert_eq!(rt.status(), crate::agents::AgentStatus::Stopped);
    }

    #[test]
    fn stop_is_idempotent() {
        let dirs = AgentDirs::from_root(PathBuf::from("X:/never"), "deepseek-harness");
        let rt = DshRuntime::new(dirs, None, None);
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
    }

    #[test]
    fn recent_log_missing_file_returns_empty() {
        let dirs = AgentDirs::from_root(PathBuf::from("X:/never"), "deepseek-harness");
        let rt = DshRuntime::new(dirs, None, None);
        assert_eq!(rt.recent_log(), String::new());
    }
}

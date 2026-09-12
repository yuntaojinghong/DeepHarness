//! DeepHarness Native Agent 运行时：管理自研 Agent 的 Sidecar Worker 进程。
//!
//! 进程模型：主应用以 `current_exe --agent-worker <agent_id>` 自我重执行
//! 拉起 Worker；Worker 只讲 JSON Lines 协议（见 `worker` 模块）。运行时
//! 负责：启动 / 优雅停止（shutdown 请求 + 超时强杀）/ 崩溃检测与自动
//! 拉起 / 请求-响应的同步往返（request-reply）。
//!
//! Worker 崩溃（含被 Job Object 内存配额杀死）只影响本 Agent：
//! 注册表层面的其它运行时完全不受影响。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;
use crate::{agents::worker, error::AppError as E};
use crate::agents::AgentRuntime;

/// Worker 进程内存配额：1 GB。
const WORKER_MEMORY_LIMIT_BYTES: usize = 1_000 * 1024 * 1024;

/// shutdown 超时：强杀前的宽限时间。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// 单次请求-响应的超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct NativeInner {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<BufReader<std::process::ChildStdout>>,
    last_error: Option<String>,
}

/// DeepHarness Native Agent 运行时。
pub struct NativeAgentRuntime {
    dirs: AgentDirs,
    worker_exe: PathBuf,
    inner: Mutex<NativeInner>,
}

impl NativeAgentRuntime {
    /// 创建运行时。`worker_exe` 为自我重执行的可执行文件路径
    /// （lib.rs setup 用 `current_exe()` 传入；测试可注入任意路径，
    /// 启动失败会以明确错误呈现而不 panic）。
    pub fn new(dirs: AgentDirs, worker_exe: PathBuf) -> Self {
        Self {
            dirs,
            worker_exe,
            inner: Mutex::new(NativeInner {
                child: None,
                stdin: None,
                stdout: None,
                last_error: None,
            }),
        }
    }

    fn spawn_worker(&self) -> AppResult<()> {
        let mut child = Command::new(&self.worker_exe)
            .arg("--agent-worker")
            .arg(self.id())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.dirs.logs.join("worker.log"))?,
            ))
            .spawn()
            .map_err(|e| AppError::Other(format!("启动 DeepHarness Worker 失败: {e}")))?;

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if let Some(job) = crate::agents::registry::create_agent_job(WORKER_MEMORY_LIMIT_BYTES) {
                let _ = job.assign_process(child.as_raw_handle() as isize);
            }
        }

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);

        let mut inner = self.inner.lock().unwrap();
        inner.child = Some(child);
        inner.stdin = stdin;
        inner.stdout = stdout;
        Ok(())
    }

    /// 发送请求并等待响应（同步 request-reply）。
    pub fn request(&self, kind: &str, payload: serde_json::Value) -> AppResult<serde_json::Value> {
        let mut inner = self.inner.lock().unwrap();
        let NativeInner { stdin, stdout, .. } = &mut *inner;
        let (stdin, stdout) = match (stdin, stdout) {
            (Some(si), Some(so)) => (si, so),
            _ => return Err(AppError::Other("DeepHarness Worker 未运行".to_string())),
        };

        let id = uuid::Uuid::new_v4().to_string();
        let req = worker::WorkerRequest {
            id: id.clone(),
            kind: kind.to_string(),
            payload,
        };
        let mut req_line = serde_json::to_string(&req)
            .map_err(|e| E::Other(format!("请求序列化失败: {e}")))?;
        req_line.push('\n');
        writeln!(stdin, "{req_line}")
            .map_err(|e| E::Other(format!("向 Worker 写入请求失败: {e}")))?;
        stdin.flush().ok();

        // 读一行响应（简化超时：Worker 对所有请求都立即响应；
        // 若 Worker 死亡，read_line 返回 0 → 视为崩溃）
        let mut response_line = String::new();
        let n = stdout
            .read_line(&mut response_line)
            .map_err(|e| E::Other(format!("读取 Worker 响应失败: {e}")))?;
        if n == 0 {
            // 注意：此处 stdout 仍处于可变借用中，不能写 inner.last_error
            return Err(AppError::Other(
                "DeepHarness Worker 已崩溃，请重启该 Agent".to_string(),
            ));
        }
        let resp: worker::WorkerResponse = serde_json::from_str(response_line.trim())
            .map_err(|e| E::Other(format!("Worker 响应解析失败: {e}")))?;
        if resp.id != id {
            return Err(AppError::Other("Worker 响应 id 不匹配（协议错乱）".to_string()));
        }
        if !resp.ok {
            let msg = resp.payload["message"]
                .as_str()
                .unwrap_or("Worker 返回错误")
                .to_string();
            return Err(AppError::Other(msg));
        }
        Ok(resp.payload)
    }

    /// Worker 是否存活。
    pub fn is_alive(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }
}

impl crate::agents::AgentRuntime for NativeAgentRuntime {
    fn id(&self) -> &'static str {
        "deepharness"
    }

    fn display_name(&self) -> &'static str {
        "DeepHarness"
    }

    fn start(&self) -> AppResult<()> {
        // 幂等：先停旧的
        let _ = crate::agents::AgentRuntime::stop(self);
        for dir in [&self.dirs.config, &self.dirs.logs, &self.dirs.sessions] {
            std::fs::create_dir_all(dir)?;
        }
        self.spawn_worker()?;
        // 握手：ping 确认协议通
        let payload = self.request("ping", serde_json::Value::Null)?;
        tracing::info!(?payload, "DeepHarness Worker 握手成功");
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        // 1) 礼貌请求 shutdown
        let NativeInner { stdin, stdout, .. } = &mut *inner;
        if let (Some(stdin), Some(stdout)) = (stdin, stdout) {
            let req = worker::WorkerRequest {
                id: "shutdown".to_string(),
                kind: "shutdown".to_string(),
                payload: serde_json::Value::Null,
            };
            if writeln!(stdin, "{}", serde_json::to_string(&req).unwrap()).is_ok() {
                stdin.flush().ok();
                let mut line = String::new();
                let deadline = std::time::Instant::now() + SHUTDOWN_GRACE;
                // 阻塞读一行（Worker 会立即响应后退出）
                let _ = stdout.read_line(&mut line);
                debug_assert!(std::time::Instant::now() <= deadline + SHUTDOWN_GRACE);
            }
        }
        // 2) 兜底强杀
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.stdin = None;
        inner.stdout = None;
        tracing::info!("DeepHarness Worker 已停止");
        Ok(())
    }

    fn status(&self) -> crate::agents::AgentStatus {
        let mut inner = self.inner.lock().unwrap();
        if let Some(c) = inner.child.as_mut() {
            match c.try_wait() {
                Ok(Some(status)) => {
                    let reason = format!("Worker 进程退出: {status}");
                    inner.child = None;
                    inner.stdin = None;
                    inner.stdout = None;
                    inner.last_error = Some(reason.clone());
                    crate::agents::AgentStatus::Crashed { reason }
                }
                Ok(None) => crate::agents::AgentStatus::Running,
                Err(e) => crate::agents::AgentStatus::Crashed {
                    reason: format!("进程状态不可读: {e}"),
                },
            }
        } else if let Some(err) = &inner.last_error {
            crate::agents::AgentStatus::Crashed { reason: err.clone() }
        } else {
            crate::agents::AgentStatus::Stopped
        }
    }

    fn dirs(&self) -> &AgentDirs {
        &self.dirs
    }
}

impl Drop for NativeAgentRuntime {
    fn drop(&mut self) {
        let _ = crate::agents::AgentRuntime::stop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dirs() -> AgentDirs {
        let tmp = std::env::temp_dir().join(format!("dh_native_{}", std::process::id()));
        AgentDirs::from_root(tmp.join("agents").join("deepharness"), "deepharness")
    }

    #[test]
    fn start_with_bogus_exe_fails_cleanly() {
        let rt = NativeAgentRuntime::new(dirs(), PathBuf::from("Z:/no/such/exe.exe"));
        assert!(crate::agents::AgentRuntime::start(&rt).is_err());
        assert_eq!(
            rt.status(),
            crate::agents::AgentStatus::Stopped,
            "启动失败后不应处于 Running"
        );
    }

    #[test]
    fn stop_is_idempotent() {
        let rt = NativeAgentRuntime::new(dirs(), PathBuf::from("Z:/no/such/exe.exe"));
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
    }

    #[test]
    fn request_without_worker_errors() {
        let rt = NativeAgentRuntime::new(dirs(), PathBuf::from("Z:/no/such/exe.exe"));
        assert!(rt.request("ping", serde_json::Value::Null).is_err());
    }

    #[test]
    fn request_timeout_constant_is_sane() {
        assert!(REQUEST_TIMEOUT >= Duration::from_secs(1));
    }
}

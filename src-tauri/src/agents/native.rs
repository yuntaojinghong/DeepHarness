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
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use crate::agents::agent_config::AgentModelConfig;
use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;
use crate::{agents::worker, error::AppError as E};
use crate::agents::AgentRuntime;

/// Worker 进程内存配额：1 GB。
const WORKER_MEMORY_LIMIT_BYTES: usize = 1_000 * 1024 * 1024;

/// shutdown 超时：强杀前的宽限时间。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// 控制类请求（ping / status / configure / 记忆操作）的响应超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 长任务请求（plan / run_task）的响应超时：模型多轮往返可能耗时数分钟。
const LONG_REQUEST_TIMEOUT: Duration = Duration::from_secs(900);

/// 协议错乱时最多丢弃多少条陈旧响应（超时后迟到的回执）。
const MAX_STALE_RESPONSES: usize = 8;

/// 按请求类型选择响应超时。
fn timeout_for(kind: &str) -> Duration {
    match kind {
        "plan" | "run_task" => LONG_REQUEST_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}

struct NativeInner {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
    /// Worker 的 stdout 由独立读取线程泵入此通道，请求方按类型超时接收，
    /// 避免 Worker 卡死时主进程（乃至 UI）被无限期阻塞。
    lines: Option<Receiver<String>>,
    last_error: Option<String>,
}

/// DeepHarness Native Agent 运行时。
pub struct NativeAgentRuntime {
    dirs: AgentDirs,
    worker_exe: PathBuf,
    /// 应用数据根目录（Worker 的 PermissionStore 直接读取
    /// `<data_root>/permissions.json`，授权状态自动同步）。
    data_root: PathBuf,
    inner: Mutex<NativeInner>,
}

impl NativeAgentRuntime {
    /// 创建运行时。`worker_exe` 为自我重执行的可执行文件路径
    /// （lib.rs setup 用 `current_exe()` 传入；测试可注入任意路径，
    /// 启动失败会以明确错误呈现而不 panic）。
    pub fn new(dirs: AgentDirs, worker_exe: PathBuf, data_root: PathBuf) -> Self {
        Self {
            dirs,
            worker_exe,
            data_root,
            inner: Mutex::new(NativeInner {
                child: None,
                stdin: None,
                lines: None,
                last_error: None,
            }),
        }
    }

    fn spawn_worker(&self) -> AppResult<()> {
        // 自我重执行：主程序本身是 GUI 子系统，但同一条命令行若被当作
        // 控制台程序拉起仍可能带出窗口，这里统一静默启动。
        let mut child = crate::process::command(&self.worker_exe)
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
        let stdout = child.stdout.take();
        // 独立读取线程：Worker 的 stdout 逐行泵入通道，主线程按超时接收。
        // 线程随管道 EOF（Worker 退出）自然结束，不泄漏。
        let lines = stdout.map(|out| {
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            let spawned = std::thread::Builder::new()
                .name("deepharness-worker-reader".to_string())
                .spawn(move || {
                    let mut reader = BufReader::new(out);
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => break, // EOF / 管道错误：Worker 已退出
                            Ok(_) => {
                                if tx.send(line).is_err() {
                                    break; // 接收端已被丢弃（运行时回收）
                                }
                            }
                        }
                    }
                });
            if let Err(e) = spawned {
                tracing::error!(error = %e, "启动 Worker 读取线程失败");
            }
            rx
        });

        let mut inner = self.inner.lock().unwrap();
        inner.child = Some(child);
        inner.stdin = stdin;
        inner.lines = lines;
        Ok(())
    }

    /// 发送请求并等待响应（同步 request-reply，带按类型超时）。
    ///
    /// 超时/管道断开都会返回明确错误并记录 `last_error`，绝不无限阻塞；
    /// 超时后迟到的陈旧响应会在后续请求中被识别并丢弃（最多
    /// `MAX_STALE_RESPONSES` 条，超出即判定协议错乱）。
    pub fn request(&self, kind: &str, payload: serde_json::Value) -> AppResult<serde_json::Value> {
        let mut inner = self.inner.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let timeout = timeout_for(kind);

        // 写入请求（借用范围独立，后续才能写 last_error）
        {
            let stdin = match inner.stdin.as_mut() {
                Some(si) => si,
                None => return Err(AppError::Other("DeepHarness Worker 未运行".to_string())),
            };
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
        }

        // 按超时接收响应，跳过不属于本次请求的陈旧回执
        let mut response_line: Option<String> = None;
        for _ in 0..MAX_STALE_RESPONSES {
            let received = {
                let lines = match inner.lines.as_ref() {
                    Some(rx) => rx,
                    None => return Err(AppError::Other("DeepHarness Worker 未运行".to_string())),
                };
                lines.recv_timeout(timeout)
            };
            match received {
                Ok(line) => {
                    // 无 id 或 id 不匹配：视为上一条超时请求的迟到回执，丢弃
                    let matched = serde_json::from_str::<worker::WorkerResponse>(line.trim())
                        .map(|r| r.id == id)
                        .unwrap_or(false);
                    if matched {
                        response_line = Some(line);
                        break;
                    }
                    tracing::warn!(kind, "丢弃与当前请求不匹配的 Worker 响应");
                }
                Err(RecvTimeoutError::Timeout) => {
                    let msg = format!(
                        "DeepHarness Worker 响应超时（{kind} 超过 {} 秒未返回），请检查网络或重启该 Agent",
                        timeout.as_secs()
                    );
                    inner.last_error = Some(msg.clone());
                    return Err(AppError::Other(msg));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let msg = "DeepHarness Worker 已崩溃（管道断开），请重启该 Agent".to_string();
                    inner.last_error = Some(msg.clone());
                    return Err(AppError::Other(msg));
                }
            }
        }
        let response_line = response_line.ok_or_else(|| {
            AppError::Other("Worker 连续返回不匹配的响应（协议错乱），请重启该 Agent".to_string())
        })?;

        let resp: worker::WorkerResponse = serde_json::from_str(response_line.trim())
            .map_err(|e| E::Other(format!("Worker 响应解析失败: {e}")))?;
        debug_assert_eq!(resp.id, id, "响应 id 已在接收阶段校验");
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

    /// 把已持久化的模型配置注入 Worker（从未配置过则静默跳过，
    /// 由上层命令通过明确错误提示用户先完成配置）。
    fn apply_saved_config(&self) -> AppResult<()> {
        if let Some(cfg) = AgentModelConfig::load(&self.dirs) {
            self.reconfigure(&cfg)?;
        }
        Ok(())
    }

    /// 向运行中的 Worker 发送 `configure`（热更新模型参数与权限视图）。
    pub fn reconfigure(&self, cfg: &AgentModelConfig) -> AppResult<serde_json::Value> {
        cfg.validate()?;
        let payload = serde_json::json!({
            "baseUrl": cfg.base_url,
            "apiKey": cfg.api_key,
            "model": cfg.model,
            "agentId": self.id(),
            "dataRoot": self.data_root.display().to_string(),
        });
        self.request("configure", payload)
    }

    /// 确保 Worker 存活：未运行则自动拉起并注入已保存的配置。
    pub fn ensure_running(&self) -> AppResult<()> {
        if !self.is_alive() {
            crate::agents::AgentRuntime::start(self)?;
        }
        Ok(())
    }

    /// Worker 的就绪状态（未运行时返回 running:false 而非报错）。
    pub fn readiness(&self) -> AppResult<serde_json::Value> {
        if !self.is_alive() {
            return Ok(serde_json::json!({ "running": false, "configured": false }));
        }
        let resp = self.request("status", serde_json::Value::Null)?;
        Ok(serde_json::json!({
            "running": true,
            "configured": resp.get("configured").and_then(|v| v.as_bool()).unwrap_or(false),
            "pid": resp.get("pid").cloned().unwrap_or(serde_json::Value::Null),
            "version": resp.get("version").cloned().unwrap_or(serde_json::Value::Null),
        }))
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
        // 注入模型配置与权限视图（已保存过配置时）
        match self.apply_saved_config() {
            Ok(()) => {}
            Err(e) => {
                // 配置注入失败不让启动失败：Worker 保持存活（未配置态），
                // 用户修复配置后经 configure 命令热注入即可。
                tracing::warn!("DeepHarness Worker 配置注入失败（未配置态运行）: {e}");
            }
        }
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        // 1) 礼貌请求 shutdown（等回执，最宽限 SHUTDOWN_GRACE）
        {
            let guard = &mut *inner;
            let stdin = guard.stdin.as_mut();
            let lines = guard.lines.as_ref();
            if let (Some(stdin), Some(lines)) = (stdin, lines) {
                let req = worker::WorkerRequest {
                    id: "shutdown".to_string(),
                    kind: "shutdown".to_string(),
                    payload: serde_json::Value::Null,
                };
                if writeln!(stdin, "{}", serde_json::to_string(&req).unwrap()).is_ok() {
                    stdin.flush().ok();
                    // Worker 会立即响应 goodbye 后自行退出
                    let _ = lines.recv_timeout(SHUTDOWN_GRACE);
                }
            }
        }
        // 2) 兜底强杀
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.stdin = None;
        inner.lines = None;
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
                    inner.lines = None;
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

    fn data_root() -> PathBuf {
        std::env::temp_dir().join(format!("dh_native_data_{}", std::process::id()))
    }

    fn runtime() -> NativeAgentRuntime {
        NativeAgentRuntime::new(dirs(), PathBuf::from("Z:/no/such/exe.exe"), data_root())
    }

    #[test]
    fn start_with_bogus_exe_fails_cleanly() {
        let rt = runtime();
        assert!(crate::agents::AgentRuntime::start(&rt).is_err());
        assert_eq!(
            rt.status(),
            crate::agents::AgentStatus::Stopped,
            "启动失败后不应处于 Running"
        );
    }

    #[test]
    fn stop_is_idempotent() {
        let rt = runtime();
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
    }

    #[test]
    fn request_without_worker_errors() {
        let rt = runtime();
        assert!(rt.request("ping", serde_json::Value::Null).is_err());
    }

    #[test]
    fn readiness_reports_not_running_for_dead_worker() {
        let rt = runtime();
        let info = rt.readiness().expect("未运行时 readiness 不应报错");
        assert_eq!(info["running"], false);
        assert_eq!(info["configured"], false);
    }

    #[test]
    fn reconfigure_rejects_incomplete_config() {
        let rt = runtime();
        let cfg = AgentModelConfig {
            base_url: "https://api.deepseek.com".to_string(),
            api_key: String::new(),
            model: "deepseek-chat".to_string(),
        };
        let err = rt.reconfigure(&cfg).expect_err("缺 apiKey 应报错");
        assert!(err.to_string().contains("不能为空"), "{err}");
    }

    #[test]
    fn reconfigure_without_worker_errors_clearly() {
        let rt = runtime();
        let cfg = AgentModelConfig {
            base_url: "https://api.deepseek.com".to_string(),
            api_key: "sk-test".to_string(),
            model: "deepseek-chat".to_string(),
        };
        let err = rt.reconfigure(&cfg).expect_err("Worker 未运行应报错");
        assert!(err.to_string().contains("Worker"), "{err}");
    }

    #[test]
    fn request_timeout_constant_is_sane() {
        assert!(REQUEST_TIMEOUT >= Duration::from_secs(1));
        assert!(LONG_REQUEST_TIMEOUT > REQUEST_TIMEOUT, "长任务超时应更宽松");
    }

    #[test]
    fn timeout_mapping_by_request_kind() {
        // 长任务走宽松超时，控制类请求走 30 秒
        assert_eq!(timeout_for("run_task"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("plan"), LONG_REQUEST_TIMEOUT);
        assert_eq!(timeout_for("ping"), REQUEST_TIMEOUT);
        assert_eq!(timeout_for("configure"), REQUEST_TIMEOUT);
        assert_eq!(timeout_for("recall"), REQUEST_TIMEOUT);
        assert_eq!(timeout_for("未知类型"), REQUEST_TIMEOUT);
    }

    #[test]
    fn stale_response_budget_is_small_and_positive() {
        assert!(MAX_STALE_RESPONSES >= 1 && MAX_STALE_RESPONSES <= 32);
    }
}

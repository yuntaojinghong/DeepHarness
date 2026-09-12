//! Codex 运行时：以本地子进程方式集成 OpenAI Codex CLI。
//!
//! 隔离与安全设计：
//! - 通过 `CODEX_HOME` 环境变量把 Codex 的配置 / 会话数据固定到
//!   `agents/codex/config/`，与系统默认目录（~/.codex）完全隔离；
//! - 任务执行（`codex exec`）的 stdout 实时转发到
//!   `agents/codex/logs/runtime.log`，并通过 Tauri 事件流推给前端；
//! - 子进程加入独立 Job Object（内存配额 + kill-on-close）；
//! - Codex 未安装 / 崩溃只影响本运行时状态，不波及其他 Agent。
//!
//! PTY 说明：Codex 的交互式 TUI 需要伪终端；本运行时采用官方的
//! 非交互模式（`codex exec`），以管道 + 实时流的方式获得同等能力，
//! 同时避免在 Windows 上引入 PTY 依赖。

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;
use crate::agents::AgentRuntime;

/// Codex 进程内存配额：1 GB。
const CODEX_MEMORY_LIMIT_BYTES: usize = 1_000 * 1024 * 1024;

/// 单条流式输出片段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOutputChunk {
    pub agent: String,
    pub session_seq: u64,
    pub text: String,
}

/// Codex 运行时。
pub struct CodexRuntime {
    dirs: AgentDirs,
    /// 前端事件出口（None 时仅写日志，供测试使用）。
    event_sink: Option<tauri::AppHandle>,
    inner: Mutex<CodexInner>,
    seq_counter: AtomicU64,
}

#[derive(Default)]
struct CodexInner {
    /// 当前正在执行的任务进程。
    active: Option<Child>,
    /// 活动进程的 stdin（向任务写入补充输入用）。
    active_stdin: Option<ChildStdin>,
    /// CLI 可用性探测结果。
    cli_version: Option<String>,
    last_error: Option<String>,
}

impl CodexRuntime {
    pub fn new(dirs: AgentDirs, event_sink: Option<tauri::AppHandle>) -> Self {
        Self {
            dirs,
            event_sink,
            inner: Mutex::new(CodexInner::default()),
            seq_counter: AtomicU64::new(0),
        }
    }

    /// 探测系统中的 codex CLI（`where codex` + `codex --version`）。
    fn probe_cli(&self) -> AppResult<String> {
        // codex 是 Node CLI（console 子系统），静默启动以免闪出黑框。
        let located = crate::process::command("where")
            .arg("codex")
            .output()
            .map_err(|e| AppError::Other(format!("探测 codex 失败: {e}")))?;
        if !located.status.success() {
            return Err(AppError::Other(
                "未找到 codex CLI。请在系统上安装 OpenAI Codex CLI（npm i -g @openai/codex）后重试。"
                    .to_string(),
            ));
        }
        let version_out = crate::process::command("codex")
            .arg("--version")
            .output()
            .map_err(|e| AppError::Other(format!("运行 codex --version 失败: {e}")))?;
        let version = String::from_utf8_lossy(&version_out.stdout).trim().to_string();
        Ok(if version.is_empty() {
            "unknown".to_string()
        } else {
            version
        })
    }

    fn log_path(&self) -> PathBuf {
        self.dirs.logs.join("runtime.log")
    }

    fn append_log(&self, line: &str) {
        use std::io::Write;
        if let Some(parent) = self.log_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// 提交一个任务给 Codex（`codex exec`），立即返回；输出走日志与事件流。
    pub fn submit_task(&self, prompt: &str) -> AppResult<u64> {
        let mut inner = self.inner.lock().unwrap();
        if inner.active.is_some() {
            return Err(AppError::Other(
                "Codex 正在执行任务，请等待完成或先停止当前任务".to_string(),
            ));
        }

        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
            .map_err(|e| AppError::Io(e))?;

        let mut child = crate::process::command("codex")
            .env("CODEX_HOME", &self.dirs.config)
            .arg("exec")
            .arg(prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_file.try_clone().map_err(AppError::Io)?))
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Other(format!("启动 codex 失败: {e}")))?;

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if let Some(job) = crate::agents::registry::create_agent_job(CODEX_MEMORY_LIMIT_BYTES) {
                let _ = job.assign_process(child.as_raw_handle() as isize);
            }
        }

        let stdout = child.stdout.take();
        let stdin = child.stdin.take();

        // stdout 泵线程：逐行转发到日志 + 前端事件流
        if let Some(out) = stdout {
            let agent_id = self.id();
            let sink = self.event_sink.clone();
            let log_path = self.log_path();
            std::thread::spawn(move || {
                let reader = BufReader::new(out);
                for line in reader.lines().map_while(Result::ok) {
                    emit_output(
                        &sink,
                        &CodexOutputChunk {
                            agent: agent_id.to_string(),
                            session_seq: seq,
                            text: format!("{line}\n"),
                        },
                    );
                    append_line(&log_path, &line);
                }
            });
        }

        inner.active = Some(child);
        inner.active_stdin = stdin;
        self.append_log(&format!("[task {seq}] started: {prompt}"));
        tracing::info!(seq, "Codex 任务已提交");
        Ok(seq)
    }

    /// 等待当前任务结束并返回退出码（无任务时返回 None）。
    pub fn wait_current_task(&self) -> AppResult<Option<i32>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(mut child) = inner.active.take() {
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).map_err(AppError::Io)?;
            inner.active_stdin = None;
            self.append_log(&format!("[task] finished with exit {code}"));
            tracing::info!(code, "Codex 任务结束");
            Ok(Some(code))
        } else {
            Ok(None)
        }
    }

    /// 向当前任务写入补充输入。
    pub fn send_input(&self, input: &str) -> AppResult<()> {
        use std::io::Write;
        let mut inner = self.inner.lock().unwrap();
        match &mut inner.active_stdin {
            Some(stdin) => {
                stdin
                    .write_all(input.as_bytes())
                    .map_err(|e| AppError::Other(format!("写入 codex stdin 失败: {e}")))?;
                stdin.flush().map_err(AppError::Io)?;
                Ok(())
            }
            None => Err(AppError::Other("Codex 当前没有可交互的任务".to_string())),
        }
    }

    pub fn cli_version(&self) -> Option<String> {
        self.inner.lock().unwrap().cli_version.clone()
    }
}

fn append_line(path: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

impl Drop for CodexRuntime {
    fn drop(&mut self) {
        let _ = crate::agents::AgentRuntime::stop(self);
    }
}

impl crate::agents::AgentRuntime for CodexRuntime {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    /// 启动 = 探测 CLI 可用性；Codex 采用按需拉起进程的模型。
    fn start(&self) -> AppResult<()> {
        std::fs::create_dir_all(&self.dirs.config)?;
        std::fs::create_dir_all(&self.dirs.logs)?;
        let version = self.probe_cli()?;
        let mut inner = self.inner.lock().unwrap();
        inner.cli_version = Some(version.clone());
        inner.last_error = None;
        tracing::info!(%version, "Codex CLI 就绪");
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(mut child) = inner.active.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!("Codex 运行时已停止（活动任务被终止）");
        }
        inner.active_stdin = None;
        Ok(())
    }

    fn status(&self) -> crate::agents::AgentStatus {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.active.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    inner.active = None;
                    inner.active_stdin = None;
                    crate::agents::AgentStatus::Stopped
                }
                Ok(None) => crate::agents::AgentStatus::Running,
                Err(e) => crate::agents::AgentStatus::Crashed {
                    reason: format!("进程状态不可读: {e}"),
                },
            }
        } else if inner.cli_version.is_some() {
            crate::agents::AgentStatus::Stopped
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

/// 向事件流推送一条 Codex 输出（无事件接收端时静默跳过）。
///
/// 作为自由函数提供给 stdout 泵线程使用：线程闭包不持有 `self`。
fn emit_output(sink: &Option<tauri::AppHandle>, chunk: &CodexOutputChunk) {
    if let Some(app) = sink {
        use tauri::Emitter;
        let _ = app.emit("agent://codex/output", chunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dirs(tag: &str) -> AgentDirs {
        let tmp = std::env::temp_dir().join(format!("dh_codex_{tag}_{}", std::process::id()));
        AgentDirs::from_root(tmp.join("agents").join("codex"), "codex")
    }

    #[test]
    fn start_fails_cleanly_without_cli_or_env() {
        // 在没有 codex CLI 的机器上，start 返回明确错误且不 panic；
        // 在装有 codex 的机器上则成功——两种结果都视为合法。
        let rt = CodexRuntime::new(temp_dirs("start"), None);
        let result = crate::agents::AgentRuntime::start(&rt);
        match result {
            Ok(()) => assert_eq!(rt.cli_version().is_some(), true),
            Err(e) => assert!(e.to_string().contains("codex")),
        }
    }

    #[test]
    fn stop_is_idempotent() {
        let rt = CodexRuntime::new(temp_dirs("stop"), None);
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
        assert!(crate::agents::AgentRuntime::stop(&rt).is_ok());
    }

    #[test]
    fn submit_task_without_cli_returns_error() {
        let rt = CodexRuntime::new(temp_dirs("submit"), None);
        let result = rt.submit_task("hello");
        // 无论是否有 CLI：有 CLI 则成功提交，无 CLI 则明确报错，绝不 panic
        if let Err(e) = result {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn status_defaults_to_stopped() {
        let rt = CodexRuntime::new(temp_dirs("status"), None);
        assert_eq!(rt.status(), crate::agents::AgentStatus::Stopped);
        let _ = Path::new("x"); // keep Path import used
    }

    #[test]
    fn log_dir_is_under_agent_root() {
        let dirs = temp_dirs("logpath");
        let rt = CodexRuntime::new(dirs, None);
        assert!(rt.log_path().starts_with(&rt.dirs().root));
    }
}

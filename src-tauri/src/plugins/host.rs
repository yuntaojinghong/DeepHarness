//! 插件执行宿主：在受沙箱约束的 `node` 子进程里运行插件工具。
//!
//! ## 协议
//!
//! 父进程（Rust，跑在 Worker 里）与子进程（`node` + `plugin-host/host.cjs`）
//! 之间用 stdin/stdout 上的 **JSON Lines** 通信。父 → 子第一行是 invoke，
//! 之后子进程可以随时反向请求宿主能力（文件读写），父进程逐条应答：
//!
//! ```text
//! 父 → 子  {"type":"invoke","pluginDir":"C:\\…","entry":"index.js",
//!           "tool":"x","args":{…},"agentId":"deepharness","workspace":"C:\\…"}
//! 子 → 父  {"type":"log","level":"info","message":"…"}
//! 子 → 父  {"type":"host","id":1,"method":"read_file","path":"C:\\…"}
//! 父 → 子  {"type":"hostResult","id":1,"ok":true,"result":"文件内容"}
//! 子 → 父  {"type":"result","ok":true,"result":{…}}
//! ```
//!
//! ## 为什么要绕这一圈
//!
//! 插件代码是不可信的第三方 JS。它所在的 node 进程以
//! `--permission --allow-fs-read=<插件目录>` 启动，**操作系统级**地碰不到
//! 用户的磁盘（实测越界读与任何写都是 `ERR_ACCESS_DENIED`）。所以插件要
//! 读写真实文件，唯一通路就是上面的 `host` 请求 —— 由 Rust 侧用
//! `PermissionStore` 按该 Agent 的白名单逐次校验，与内置工具同一道闸门。
//!
//! shim 源码以 `include_str!` 内嵌在二进制里（见 [`HOST_SHIM_SOURCE`]），
//! 运行时落地到 `<data>/plugin-host/host.cjs`：不依赖 Tauri 资源打包，
//! 避免"开发能跑、打包缺文件"。

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::native::tools::{ToolContext, authorized};

use super::PluginTool;

/// 单次插件调用的默认超时。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// 子进程反向请求宿主的次数上限，防止插件用无限请求把调用卡死。
const MAX_HOST_CALLS: usize = 256;

/// 子进程 stdout 的行数上限（防止刷屏式输出把内存吃光）。
const MAX_LINES: usize = 10_000;

/// 子进程 stderr 保留的字节上限（仅用于报错时展示）。
const MAX_STDERR: usize = 8 * 1024;

/// 日志消息的长度上限。
const MAX_LOG_LEN: usize = 2_000;

/// 随包内置的插件宿主 shim 源码。
///
/// 用 `include_str!` 内嵌而不是走 Tauri 资源目录：`resources/*` 在
/// `.gitignore` 里被排除，走资源会出现"开发能跑、打包缺文件"。
pub const HOST_SHIM_SOURCE: &str = include_str!("../../plugin-host/host.cjs");

/// 宿主 shim 落地的子目录名（位于应用数据根目录下）。
pub const SHIM_DIR: &str = "plugin-host";

/// 把内嵌的 shim 落地到 `<data_root>/plugin-host/host.cjs`，返回其路径。
///
/// 只在内容变化时重写（升级软件后自动同步），失败返回可读错误而非 panic。
pub fn materialize_shim(data_root: &Path) -> AppResult<PathBuf> {
    let dir = data_root.join(SHIM_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("host.cjs");
    let needs_write = match std::fs::read_to_string(&path) {
        Ok(existing) => existing != HOST_SHIM_SOURCE,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&path, HOST_SHIM_SOURCE.as_bytes())?;
        tracing::info!(path = %path.display(), "插件宿主 shim 已落地");
    }
    Ok(path)
}

/// 一次插件工具调用。
pub struct Invocation<'a> {
    pub tool: &'a PluginTool,
    pub args: &'a Value,
}

/// 子进程 → 父进程的消息。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HostMessage {
    Log {
        #[serde(default)]
        level: String,
        #[serde(default)]
        message: String,
    },
    Host {
        id: u64,
        method: String,
        #[serde(default)]
        path: String,
        #[serde(default)]
        contents: Option<String>,
    },
    /// 正常结束。`#[serde(rename = "result")]` 由 rename_all 提供。
    Result {
        ok: bool,
        #[serde(default)]
        result: Value,
        #[serde(default)]
        error: Option<String>,
    },
    Fatal {
        #[serde(default)]
        error: String,
    },
}

/// 插件宿主：持有随包 node 与 shim 的路径。
#[derive(Debug, Clone)]
pub struct PluginHost {
    node: PathBuf,
    shim: PathBuf,
    timeout: Duration,
}

impl PluginHost {
    pub fn new(node: PathBuf, shim: PathBuf) -> Self {
        Self {
            node,
            shim,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// 覆盖默认超时（测试用）。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn node(&self) -> &Path {
        &self.node
    }

    pub fn shim(&self) -> &Path {
        &self.shim
    }

    /// 执行一次插件工具调用。
    ///
    /// 任何失败（超时、崩溃、协议错乱、插件抛错）都返回可读的
    /// `AppError`，绝不 panic —— 调用方（TaskRunner）会把它当成一次
    /// 普通的工具失败继续走反思流程。
    pub fn invoke(&self, inv: &Invocation<'_>, ctx: &ToolContext<'_>) -> AppResult<Value> {
        self.check_available()?;

        let plugin_dir = crate::permissions::canonicalize_lenient(&inv.tool.plugin_dir)
            .ok_or_else(|| {
                AppError::PathNotFound(inv.tool.plugin_dir.display().to_string())
            })?;

        let mut child = crate::process::command(&self.node)
            .arg("--permission")
            .arg(format!("--allow-fs-read={}", plugin_dir.display()))
            .arg("--no-warnings")
            .arg(&self.shim)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Other(format!("启动插件宿主进程失败（{}）: {e}", self.node.display()))
            })?;

        // stderr 单独收集，仅用于失败时给出原因
        let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(mut err) = child.stderr.take() {
            let sink = stderr_buf.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = err.read_to_end(&mut buf);
                let text = String::from_utf8_lossy(&buf).to_string();
                let trimmed: String = text.chars().take(MAX_STDERR).collect();
                if let Ok(mut g) = sink.lock() {
                    g.push_str(&trimmed);
                }
            });
        }

        let stderr_tail = || -> String {
            stderr_buf
                .lock()
                .map(|g| g.trim().to_string())
                .unwrap_or_default()
        };

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Other("无法获取插件宿主 stdin".to_string()))?;

        // stdout 交给独立线程逐行推送，主线程用 recv_timeout 实现整体超时；
        // 直接在管道上 read_line 会被一个卡死的插件永久挂住。
        let (tx, rx) = mpsc::channel::<String>();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Other("无法获取插件宿主 stdout".to_string()))?;
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let invoke_line = json!({
            "type": "invoke",
            "pluginDir": inv.tool.plugin_dir.display().to_string(),
            "entry": inv.tool.entry,
            "tool": inv.tool.tool.name,
            "plugin": inv.tool.plugin_id,
            "args": inv.args,
            "agentId": ctx.dirs.agent_id,
            "workspace": ctx.dirs.workspace.display().to_string(),
        })
        .to_string();

        if let Err(e) = write_line(&mut stdin, &invoke_line) {
            let _ = child.kill();
            return Err(AppError::Other(format!(
                "向插件宿主写入调用请求失败: {e}{}",
                stderr_suffix(&stderr_tail())
            )));
        }

        let deadline = Instant::now() + self.timeout;
        let mut host_calls = 0usize;
        let mut lines_seen = 0usize;
        let outcome: AppResult<Value> = loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break Err(AppError::Other(format!(
                    "插件工具 `{}` 执行超时（{} 秒）",
                    inv.tool.tool.name,
                    self.timeout.as_secs()
                )));
            };

            let line = match rx.recv_timeout(remaining) {
                Ok(l) => l,
                Err(_) => {
                    break Err(AppError::Other(format!(
                        "插件工具 `{}` 执行超时（{} 秒）{}",
                        inv.tool.tool.name,
                        self.timeout.as_secs(),
                        stderr_suffix(&stderr_tail())
                    )));
                }
            };

            lines_seen += 1;
            if lines_seen > MAX_LINES {
                break Err(AppError::Other(format!(
                    "插件 `{}` 输出行数过多，已中止",
                    inv.tool.plugin_id
                )));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: HostMessage = match serde_json::from_str(trimmed) {
                Ok(m) => m,
                Err(e) => {
                    // 子进程可能把警告打到 stdout；插件的真实输出格式由 shim
                    // 掌控，所以这里只记录不中断（真出问题会走到超时）。
                    tracing::warn!(error = %e, line = %trimmed.chars().take(200).collect::<String>(), "忽略无法解析的插件宿主输出");
                    continue;
                }
            };

            match msg {
                HostMessage::Log { level, message } => {
                    let short: String = message.chars().take(MAX_LOG_LEN).collect();
                    match level.as_str() {
                        "error" => tracing::error!(plugin = %inv.tool.plugin_id, "{short}"),
                        "warn" => tracing::warn!(plugin = %inv.tool.plugin_id, "{short}"),
                        _ => tracing::info!(plugin = %inv.tool.plugin_id, "{short}"),
                    }
                }
                HostMessage::Host {
                    id,
                    method,
                    path,
                    contents,
                } => {
                    host_calls += 1;
                    if host_calls > MAX_HOST_CALLS {
                        break Err(AppError::Other(format!(
                            "插件 `{}` 对宿主的请求次数过多（上限 {MAX_HOST_CALLS}）",
                            inv.tool.plugin_id
                        )));
                    }
                    let reply = match host_dispatch(ctx, &method, &path, contents.as_deref()) {
                        Ok(v) => json!({ "type": "hostResult", "id": id, "ok": true, "result": v }),
                        Err(e) => {
                            json!({ "type": "hostResult", "id": id, "ok": false, "error": e.to_string() })
                        }
                    };
                    if let Err(e) = write_line(&mut stdin, &reply.to_string()) {
                        break Err(AppError::Other(format!(
                            "回应插件宿主调用失败: {e}"
                        )));
                    }
                }
                HostMessage::Result { ok, result, error } => {
                    if ok {
                        break Ok(result);
                    }
                    break Err(AppError::Other(format!(
                        "插件工具 `{}` 执行失败: {}",
                        inv.tool.tool.name,
                        error.unwrap_or_else(|| "插件未提供错误信息".to_string())
                    )));
                }
                HostMessage::Fatal { error } => {
                    break Err(AppError::Other(format!(
                        "插件 `{}` 加载失败: {}{}",
                        inv.tool.plugin_id,
                        error,
                        stderr_suffix(&stderr_tail())
                    )));
                }
            }
        };

        // 无论成败都要收掉子进程：结果已拿到时它通常会自行退出，
        // 超时 / 出错路径上必须显式 kill，否则会留下孤儿 node。
        let _ = child.kill();
        let _ = child.wait();

        outcome
    }

    /// 宿主能力是否就绪。
    pub fn check_available(&self) -> AppResult<()> {
        if !self.node.is_file() {
            return Err(AppError::Other(format!(
                "随包 Node 不存在：{}",
                self.node.display()
            )));
        }
        if !self.shim.is_file() {
            return Err(AppError::Other(format!(
                "插件宿主脚本不存在：{}",
                self.shim.display()
            )));
        }
        Ok(())
    }
}

/// 处理插件对宿主的一次能力请求（文件访问一律过白名单）。
fn host_dispatch(
    ctx: &ToolContext<'_>,
    method: &str,
    path: &str,
    contents: Option<&str>,
) -> AppResult<Value> {
    match method {
        "read_file" => {
            if path.trim().is_empty() {
                return Err(AppError::Other("path 不能为空".to_string()));
            }
            let target = authorized(ctx, path, false)?;
            if !target.is_file() {
                return Err(AppError::PathNotFound(path.to_string()));
            }
            let bytes = std::fs::read(&target)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                AppError::Other(format!("`{path}` 不是 UTF-8 文本文件"))
            })?;
            Ok(Value::String(text))
        }
        "write_file" => {
            if path.trim().is_empty() {
                return Err(AppError::Other("path 不能为空".to_string()));
            }
            let contents =
                contents.ok_or_else(|| AppError::Other("write_file 缺少 contents".to_string()))?;
            let target = authorized(ctx, path, true)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, contents.as_bytes())?;
            tracing::info!(path = %path, bytes = contents.len(), "插件经宿主写入文件");
            Ok(json!({ "path": path, "written": contents.as_bytes().len() }))
        }
        "list_dir" => {
            if path.trim().is_empty() {
                return Err(AppError::Other("path 不能为空".to_string()));
            }
            let target = authorized(ctx, path, false)?;
            if !target.is_dir() {
                return Err(AppError::PathNotFound(path.to_string()));
            }
            let mut entries: Vec<Value> = Vec::new();
            for entry in std::fs::read_dir(&target)? {
                let entry = entry?;
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let size = if is_dir {
                    0
                } else {
                    entry.metadata().map(|m| m.len()).unwrap_or(0)
                };
                entries.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "isDir": is_dir,
                    "size": size,
                }));
            }
            entries.sort_by(|a, b| {
                let da = a["isDir"].as_bool().unwrap_or(false);
                let db = b["isDir"].as_bool().unwrap_or(false);
                db.cmp(&da).then_with(|| {
                    a["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
                })
            });
            Ok(json!({ "path": path, "entries": entries }))
        }
        other => Err(AppError::Other(format!(
            "插件请求了未知的宿主能力 `{other}`（仅支持 read_file / write_file / list_dir）"
        ))),
    }
}

fn write_line(stdin: &mut impl Write, line: &str) -> std::io::Result<()> {
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn stderr_suffix(tail: &str) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!("\n宿主进程 stderr: {tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AgentDirs;
    use crate::permissions::{AccessMode, PermissionStore};

    /// 宿主能力检查的重启：node / shim 路径不存在时必须给出明确错误，
    /// 而不是让调用方以为插件本身有问题。
    #[test]
    fn missing_host_files_report_clearly() {
        let host = PluginHost::new(
            PathBuf::from("Z:/nope/node.exe"),
            PathBuf::from("Z:/nope/host.cjs"),
        );
        let err = host.check_available().unwrap_err().to_string();
        assert!(err.contains("随包 Node 不存在"), "错误信息不得误导: {err}");

        // 只有 node 存在时，应报 shim 缺失
        let tmp = tempfile::tempdir().unwrap();
        let fake_node = tmp.path().join("node.exe");
        std::fs::write(&fake_node, b"x").unwrap();
        let host = PluginHost::new(fake_node, tmp.path().join("host.cjs"));
        let err = host.check_available().unwrap_err().to_string();
        assert!(err.contains("宿主脚本不存在"), "错误信息不得误导: {err}");
    }

    #[test]
    fn host_dispatch_refuses_unlisted_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("agent");
        let dirs = AgentDirs::from_root(root.clone(), "deepharness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let perms = PermissionStore::new(root);
        let ctx = ToolContext {
            perms: &perms,
            dirs: &dirs,
            plugins: None,
        };

        let outside = tmp.path().join("secret.txt");
        std::fs::write(&outside, "classified").unwrap();

        // 未授权：读 / 写 / 列目录都必须被拒
        let p = outside.display().to_string();
        for method in ["read_file", "write_file", "list_dir"] {
            let err = host_dispatch(&ctx, method, &p, Some("x")).unwrap_err();
            assert!(
                matches!(err, AppError::PathNotAuthorized(_)),
                "{method} 未授权路径应被拒，实际: {err}"
            );
        }

        // workspace 内可读
        let inside = dirs.workspace.join("ok.txt");
        std::fs::write(&inside, "hello").unwrap();
        let v = host_dispatch(&ctx, "read_file", &inside.display().to_string(), None).unwrap();
        assert_eq!(v, Value::String("hello".to_string()));

        // 显式授权后，workspace 之外也可读
        perms
            .grant("deepharness", &outside, AccessMode::Read, None)
            .unwrap();
        let v = host_dispatch(&ctx, "read_file", &p, None).unwrap();
        assert_eq!(v, Value::String("classified".to_string()));

        // 只授了读，写仍应被拒
        let err = host_dispatch(&ctx, "write_file", &p, Some("x")).unwrap_err();
        assert!(matches!(err, AppError::PathNotAuthorized(_)));
    }

    #[test]
    fn host_dispatch_rejects_unknown_method_and_blank_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("agent");
        let dirs = AgentDirs::from_root(root.clone(), "deepharness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let perms = PermissionStore::new(root);
        let ctx = ToolContext {
            perms: &perms,
            dirs: &dirs,
            plugins: None,
        };

        let err = host_dispatch(&ctx, "run_shell", "/x", None).unwrap_err();
        assert!(err.to_string().contains("未知的宿主能力"), "{err}");

        let err = host_dispatch(&ctx, "read_file", "   ", None).unwrap_err();
        assert!(err.to_string().contains("不能为空"), "{err}");

        let err = host_dispatch(&ctx, "write_file", "/x", None).unwrap_err();
        assert!(err.to_string().contains("contents"), "{err}");
    }

    #[test]
    fn host_message_parsing_is_strict_about_type() {
        let m: HostMessage = serde_json::from_str(r#"{"type":"result","ok":true,"result":42}"#).unwrap();
        assert!(matches!(m, HostMessage::Result { ok: true, .. }));

        let m: HostMessage =
            serde_json::from_str(r#"{"type":"host","id":7,"method":"read_file","path":"C:/a"}"#)
                .unwrap();
        match m {
            HostMessage::Host { id, method, path, .. } => {
                assert_eq!(id, 7);
                assert_eq!(method, "read_file");
                assert_eq!(path, "C:/a");
            }
            _ => panic!("应解析为 Host"),
        }

        let m: HostMessage = serde_json::from_str(r#"{"type":"log","message":"hi"}"#).unwrap();
        assert!(matches!(m, HostMessage::Log { .. }));

        // 未知类型必须解析失败（而不是被当成成功）
        assert!(serde_json::from_str::<HostMessage>(r#"{"type":"whatever"}"#).is_err());
    }

    /// 内嵌的 shim 必须随二进制一起到位，且包含协议的全部关键标记 ——
    /// 防止 `include_str!` 路径写错（空文件 / 拼错目录）在运行期才暴露。
    #[test]
    fn embedded_shim_is_present_and_complete() {
        for marker in [
            "\"type\":\"invoke\"",
            "hostResult",
            "read_file",
            "write_file",
            "list_dir",
            "module.exports",
            "--permission",
        ] {
            assert!(
                HOST_SHIM_SOURCE.contains(marker),
                "shim 源码缺少关键标记 {marker}"
            );
        }
        assert!(HOST_SHIM_SOURCE.len() > 2_000, "shim 太短，疑似被截断");
        assert!(
            HOST_SHIM_SOURCE.contains('{') && HOST_SHIM_SOURCE.contains('}'),
            "shim 不成形"
        );
    }

    /// 落地 shim：首次写入、重复调用幂等、内容不一致时重写（升级同步）。
    #[test]
    fn materialize_shim_writes_and_syncs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = materialize_shim(tmp.path()).unwrap();
        assert_eq!(path.file_name().unwrap(), "host.cjs");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), HOST_SHIM_SOURCE);

        // 幂等：内容一致时保持不动（用只读属性会把重写暴露成错误，
        // 这里退而断言字节完全相同）
        let again = materialize_shim(tmp.path()).unwrap();
        assert_eq!(again, path);
        assert_eq!(std::fs::read_to_string(&again).unwrap(), HOST_SHIM_SOURCE);

        // 内容被外部改动 → 下一次调用恢复为内嵌版本
        std::fs::write(&path, b"// tampered").unwrap();
        let _ = materialize_shim(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), HOST_SHIM_SOURCE);
    }
}

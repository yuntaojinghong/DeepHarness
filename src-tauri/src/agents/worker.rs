//! DeepHarness Native Agent 的 Sidecar Worker 协议。
//!
//! 进程隔离模型：主应用通过**自我重执行**（`current_exe --agent-worker`）
//! 拉起一个独立的 Worker 进程，主进程与 Worker 之间以 stdin/stdout 上的
//! JSON Lines 通信。Worker 崩溃 / 卡死 / 内存超限（Job Object 配额）只
//! 影响自研 Agent 自己，主进程检测到管道断裂即可重启 Worker。
//!
//! 协议消息（阶段 3 定义基础协议；阶段 4 的规划 / 执行 / 记忆请求
//! 在此基础上扩展 `kind` 类型）：
//!
//! 请求：  {"id":"u1","kind":"ping"}
//! 响应：  {"id":"u1","ok":true,"kind":"pong","payload":null}
//! 错误：  {"id":"u1","ok":false,"kind":"error","payload":{"message":"..."}}

use serde::{Deserialize, Serialize};

/// Worker 请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct WorkerRequest {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// Worker 响应。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerResponse {
    pub id: String,
    pub ok: bool,
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl WorkerResponse {
    pub fn success(id: &str, kind: &str, payload: serde_json::Value) -> Self {
        Self {
            id: id.to_string(),
            ok: true,
            kind: kind.to_string(),
            payload,
        }
    }

    pub fn failure(id: &str, message: &str) -> Self {
        Self {
            id: id.to_string(),
            ok: false,
            kind: "error".to_string(),
            payload: serde_json::json!({ "message": message }),
        }
    }
}

/// 把一行文本解析为请求；格式非法时返回可读错误（不 panic）。
pub fn parse_request(line: &str) -> Result<WorkerRequest, String> {
    let req: WorkerRequest = serde_json::from_str(line)
        .map_err(|e| format!("请求解析失败: {e}"))?;
    if req.id.trim().is_empty() {
        return Err("请求 id 不能为空".to_string());
    }
    if req.kind.trim().is_empty() {
        return Err("请求 kind 不能为空".to_string());
    }
    Ok(req)
}

/// 把响应序列化为一行 JSON（带换行符）。
pub fn format_response(resp: &WorkerResponse) -> String {
    format!("{}\n", serde_json::to_string(resp).unwrap_or_else(|_| {
        // 序列化失败时降级为最小错误响应，保证管道不中断
        r#"{"id":"internal","ok":false,"kind":"error","payload":{"message":"response serialization failed"}}"#.to_string()
    }))
}

/// Worker 主循环：从 `reader` 逐行读取请求、分发、写回响应。
///
/// 返回原因：收到 `shutdown` 或输入流结束（EOF，即主进程死亡）。
pub fn run_worker_loop(
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
) -> WorkerExit {
    loop {
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => return WorkerExit::Eof,
            Ok(_) => {}
            Err(_) => return WorkerExit::Eof,
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req = match parse_request(trimmed) {
            Ok(r) => r,
            Err(msg) => {
                // 无法定位 id 时用 "unknown" 兜底，主进程按错误日志处理
                let resp = WorkerResponse::failure("unknown", &msg);
                if write_line(output, &format_response(&resp)).is_err() {
                    return WorkerExit::PipeBroken;
                }
                continue;
            }
        };
        match handle_request(&req) {
            WorkerAction::Respond(resp) => {
                if write_line(output, &format_response(&resp)).is_err() {
                    return WorkerExit::PipeBroken;
                }
            }
            WorkerAction::Shutdown(resp) => {
                let _ = write_line(output, &format_response(&resp));
                return WorkerExit::Requested;
            }
        }
    }
}

/// Worker 退出原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerExit {
    /// 主进程关闭了管道（正常关闭或主进程死亡）。
    Eof,
    /// Worker 主动收到 shutdown。
    Requested,
    /// 写回响应失败（管道断裂）。
    PipeBroken,
}

/// 请求处理动作。
enum WorkerAction {
    Respond(WorkerResponse),
    Shutdown(WorkerResponse),
}

/// 处理单个请求。阶段 3 支持 ping / echo / status / shutdown；
/// 阶段 4 的规划器、执行器、记忆系统以新的 `kind` 接入。
fn handle_request(req: &WorkerRequest) -> WorkerAction {
    match req.kind.as_str() {
        "ping" => WorkerAction::Respond(WorkerResponse::success(
            &req.id,
            "pong",
            serde_json::json!({ "agent": "deepharness", "pid": std::process::id() }),
        )),
        "echo" => WorkerAction::Respond(WorkerResponse::success(
            &req.id,
            "echo",
            req.payload.clone(),
        )),
        "status" => WorkerAction::Respond(WorkerResponse::success(
            &req.id,
            "status",
            serde_json::json!({
                "agent": "deepharness",
                "pid": std::process::id(),
                "version": env!("CARGO_PKG_VERSION"),
            }),
        )),
        "shutdown" => WorkerAction::Shutdown(WorkerResponse::success(
            &req.id,
            "bye",
            serde_json::Value::Null,
        )),
        other => WorkerAction::Respond(WorkerResponse::failure(
            &req.id,
            &format!("未知请求类型: {other}"),
        )),
    }
}

fn write_line(output: &mut dyn std::io::Write, line: &str) -> std::io::Result<()> {
    output.write_all(line.as_bytes())?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::sync::{Arc, Mutex};

    /// 可跨线程读取的内存缓冲，用于断言 Worker 输出。
    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn request_roundtrip() {
        let line = r#"{"id":"u1","kind":"ping"}"#;
        let req = parse_request(line).unwrap();
        assert_eq!(req.id, "u1");
        assert_eq!(req.kind, "ping");
    }

    #[test]
    fn request_defaults_payload_to_null() {
        let req = parse_request(r#"{"id":"a","kind":"echo"}"#).unwrap();
        assert_eq!(req.payload, serde_json::Value::Null);
    }

    #[test]
    fn rejects_malformed_and_blank_ids() {
        assert!(parse_request("not json").is_err());
        assert!(parse_request(r#"{"id":"","kind":"ping"}"#).is_err());
        assert!(parse_request(r#"{"id":"x","kind":""}"#).is_err());
    }

    #[test]
    fn ping_pong_loop() {
        let input = "not json\n{\"id\":\"1\",\"kind\":\"ping\"}\n";
        let mut reader = BufReader::new(input.as_bytes());
        let out_buf = SharedBuf(Arc::new(Mutex::new(Vec::new())));
        let mut out = out_buf.clone();
        let exit = run_worker_loop(&mut reader, &mut out);
        assert_eq!(exit, WorkerExit::Eof);

        let raw = String::from_utf8(out_buf.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);

        let err: WorkerResponse = serde_json::from_str(lines[0]).unwrap();
        assert!(!err.ok);
        assert_eq!(err.id, "unknown");

        let pong: WorkerResponse = serde_json::from_str(lines[1]).unwrap();
        assert!(pong.ok);
        assert_eq!(pong.kind, "pong");
        assert_eq!(pong.payload["agent"], "deepharness");
    }

    #[test]
    fn echo_returns_payload() {
        let input = r#"{"id":"e1","kind":"echo","payload":{"n":42}}"#;
        let mut reader = BufReader::new(input.as_bytes());
        let out_buf = SharedBuf(Arc::new(Mutex::new(Vec::new())));
        let mut out = out_buf.clone();
        let exit = run_worker_loop(&mut reader, &mut out);
        assert_eq!(exit, WorkerExit::Eof);
        let raw = String::from_utf8(out_buf.0.lock().unwrap().clone()).unwrap();
        let resp: WorkerResponse = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(resp.payload["n"], 42);
    }

    #[test]
    fn shutdown_exits_cleanly_after_reply() {
        let input = r#"{"id":"s1","kind":"shutdown"}"#
            .to_string()
            + "\n"
            + r#"{"id":"s2","kind":"ping"}"# + "\n";
        let mut reader = BufReader::new(input.as_bytes());
        let out_buf = SharedBuf(Arc::new(Mutex::new(Vec::new())));
        let mut out = out_buf.clone();
        let exit = run_worker_loop(&mut reader, &mut out);
        assert_eq!(exit, WorkerExit::Requested);
        let raw = String::from_utf8(out_buf.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 1, "shutdown 后不应再处理后续请求");
        let bye: WorkerResponse = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(bye.kind, "bye");
    }

    #[test]
    fn unknown_kind_returns_error_not_crash() {
        let req = parse_request(r#"{"id":"k1","kind":"warp_drive"}"#).unwrap();
        match handle_request(&req) {
            WorkerAction::Respond(resp) => {
                assert!(!resp.ok);
                assert!(resp.payload["message"].as_str().unwrap().contains("warp_drive"));
            }
            _ => panic!("未知类型应返回错误响应"),
        }
    }
}

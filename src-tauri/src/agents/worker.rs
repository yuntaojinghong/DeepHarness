//! DeepHarness Native Agent 的 Sidecar Worker 协议。
//!
//! 进程隔离模型：主应用通过**自我重执行**（`current_exe --agent-worker`）
//! 拉起一个独立的 Worker 进程，主进程与 Worker 之间以 stdin/stdout 上的
//! JSON Lines 通信。Worker 崩溃 / 卡死 / 内存超限（Job Object 配额）只
//! 影响自研 Agent 自己，主进程检测到管道断裂即可重启 Worker。
//!
//! 协议消息：
//!
//! 请求：  {"id":"u1","kind":"ping","payload":...}
//! 响应：  {"id":"u1","ok":true,"kind":"pong","payload":...}
//! 错误：  {"id":"u1","ok":false,"kind":"error","payload":{"message":"..."}}
//!
//! 支持的 `kind`：
//! - `ping` / `echo` / `status` / `shutdown`：基础协议；
//! - `configure`：注入模型提供方参数 + Agent 目录布局（含权限白名单，
//!   直接读取主进程维护的 permissions.json，授权状态自动同步）；
//! - `plan`：目标 → 结构化 JSON 计划；
//! - `run_task`：完整任务循环（规划 → 执行 → 反思 → 记忆沉淀）；
//! - `remember` / `recall` / `forget` / `memory_stats`：长期记忆操作。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::native::executor::TaskRunner;
use crate::native::memory::{MemoryStore, MemoryStats, Memory};
use crate::native::model::{DeepSeekProvider, ModelProvider};
use crate::native::planner::{Plan, Planner};
use crate::native::tools::ToolContext;
use crate::paths::{ensure_agent_dirs, validate_agent_id, AgentDirs};
use crate::permissions::PermissionStore;

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

/// Worker 运行期状态（configure 之后才可用）。
pub struct WorkerState {
    provider: Option<std::sync::Arc<dyn ModelProvider>>,
    memory: Option<MemoryStore>,
    perms: Option<PermissionStore>,
    dirs: Option<AgentDirs>,
}

impl WorkerState {
    /// 初始状态：全部未配置。
    pub fn new() -> Self {
        Self {
            provider: None,
            memory: None,
            perms: None,
            dirs: None,
        }
    }

    /// 是否已 configure。
    pub fn is_configured(&self) -> bool {
        self.provider.is_some() && self.memory.is_some() && self.perms.is_some() && self.dirs.is_some()
    }

    fn require_provider(&self) -> AppResult<std::sync::Arc<dyn ModelProvider>> {
        self.provider
            .clone()
            .ok_or_else(|| AppError::Other("Worker 尚未 configure（缺少模型提供方）".to_string()))
    }

    fn require_memory(&self) -> AppResult<&MemoryStore> {
        self.memory
            .as_ref()
            .ok_or_else(|| AppError::Other("Worker 尚未 configure（缺少记忆库）".to_string()))
    }

    fn require_tools(&self) -> AppResult<ToolContext<'_>> {
        match (&self.perms, &self.dirs) {
            (Some(perms), Some(dirs)) => Ok(ToolContext { perms, dirs }),
            _ => Err(AppError::Other("Worker 尚未 configure（缺少工具上下文）".to_string())),
        }
    }
}

impl Default for WorkerState {
    fn default() -> Self {
        Self::new()
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
    state: &mut WorkerState,
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
        match handle_request(&req, state) {
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

/// configure 请求的载荷。
#[derive(Debug, Deserialize)]
struct ConfigurePayload {
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    model: String,
    #[serde(rename = "agentId")]
    agent_id: String,
    #[serde(rename = "dataRoot")]
    data_root: String,
}

/// 处理单个请求。
fn handle_request(req: &WorkerRequest, state: &mut WorkerState) -> WorkerAction {
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
                "configured": state.is_configured(),
            }),
        )),
        "shutdown" => WorkerAction::Shutdown(WorkerResponse::success(
            &req.id,
            "bye",
            serde_json::Value::Null,
        )),
        "configure" => match handle_configure(&req.payload, state) {
            Ok(info) => WorkerAction::Respond(WorkerResponse::success(&req.id, "configured", info)),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "plan" => match handle_plan(&req.payload, state) {
            Ok(plan) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "plan",
                serde_json::to_value(&plan).unwrap_or(serde_json::Value::Null),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "run_task" => match handle_run_task(&req.payload, state) {
            Ok(outcome) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "task_outcome",
                serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "remember" => match handle_remember(&req.payload, state) {
            Ok(id) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "remembered",
                serde_json::json!({ "id": id }),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "recall" => match handle_recall(&req.payload, state) {
            Ok(list) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "memories",
                serde_json::to_value(&list).unwrap_or(serde_json::Value::Null),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "forget" => match handle_forget(&req.payload, state) {
            Ok(existed) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "forgotten",
                serde_json::json!({ "existed": existed }),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        "memory_stats" => match state.require_memory().and_then(|m| m.stats()) {
            Ok(stats) => WorkerAction::Respond(WorkerResponse::success(
                &req.id,
                "memory_stats",
                serde_json::to_value(&stats).unwrap_or(serde_json::Value::Null),
            )),
            Err(e) => WorkerAction::Respond(WorkerResponse::failure(&req.id, &e.to_string())),
        },
        other => WorkerAction::Respond(WorkerResponse::failure(
            &req.id,
            &format!("未知请求类型: {other}"),
        )),
    }
}

fn handle_configure(payload: &serde_json::Value, state: &mut WorkerState) -> AppResult<serde_json::Value> {
    let cfg: ConfigurePayload = serde_json::from_value(payload.clone())
        .map_err(|e| AppError::Other(format!("configure 载荷无效: {e}")))?;
    validate_agent_id(&cfg.agent_id)?;
    if cfg.base_url.trim().is_empty() || cfg.api_key.trim().is_empty() || cfg.model.trim().is_empty() {
        return Err(AppError::Other("configure 缺少 baseUrl / apiKey / model".to_string()));
    }
    let data_root = std::path::PathBuf::from(&cfg.data_root);
    let dirs = ensure_agent_dirs(&data_root, &cfg.agent_id)?;
    // 直接读取主进程维护的 permissions.json —— 授权状态自动同步
    let perms = PermissionStore::new(data_root.clone());
    let memory = MemoryStore::open(&dirs.config.join("memory.db"))?;
    let provider: std::sync::Arc<dyn ModelProvider> = std::sync::Arc::new(
        DeepSeekProvider::new(cfg.base_url.clone(), cfg.api_key.clone(), cfg.model.clone()),
    );

    state.provider = Some(provider);
    state.memory = Some(memory);
    state.perms = Some(perms);
    state.dirs = Some(dirs);

    tracing::info!(agent = %cfg.agent_id, model = %cfg.model, "Worker 已 configure");
    Ok(serde_json::json!({
        "agent": cfg.agent_id,
        "model": cfg.model,
        "workspace": state.dirs.as_ref().map(|d| d.workspace.display().to_string()),
    }))
}

fn handle_plan(payload: &serde_json::Value, state: &WorkerState) -> AppResult<Plan> {
    let goal = payload
        .get("goal")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("缺少 goal".to_string()))?;
    let context = payload
        .get("context")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let provider = state.require_provider()?;
    Planner::new(provider).make_plan(goal, context)
}

fn handle_run_task(payload: &serde_json::Value, state: &WorkerState) -> AppResult<crate::native::executor::TaskOutcome> {
    let goal = payload
        .get("goal")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("缺少 goal".to_string()))?;
    let context = payload
        .get("context")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let provider = state.require_provider()?;
    let ctx = state.require_tools()?;
    let memory = state.require_memory()?;
    TaskRunner::new(provider, ctx, memory).run(goal, context)
}

fn handle_remember(payload: &serde_json::Value, state: &WorkerState) -> AppResult<i64> {
    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("缺少 kind".to_string()))?;
    let content = payload
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("缺少 content".to_string()))?;
    let tags: Vec<&str> = payload
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|t| t.as_str()).collect())
        .unwrap_or_default();
    let importance = payload
        .get("importance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5);
    state.require_memory()?.remember(kind, content, &tags, importance)
}

fn handle_recall(payload: &serde_json::Value, state: &WorkerState) -> AppResult<Vec<Memory>> {
    let query = payload
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Other("缺少 query".to_string()))?;
    let limit = payload
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;
    state.require_memory()?.recall(query, limit)
}

fn handle_forget(payload: &serde_json::Value, state: &WorkerState) -> AppResult<bool> {
    let id = payload
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::Other("缺少 id".to_string()))?;
    state.require_memory()?.forget(id)
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
        let mut state = WorkerState::new();
        let exit = run_worker_loop(&mut reader, &mut out, &mut state);
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
        let mut state = WorkerState::new();
        let exit = run_worker_loop(&mut reader, &mut out, &mut state);
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
        let mut state = WorkerState::new();
        let exit = run_worker_loop(&mut reader, &mut out, &mut state);
        assert_eq!(exit, WorkerExit::Requested);
        let raw = String::from_utf8(out_buf.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 1, "shutdown 后不应再处理后续请求");
        let bye: WorkerResponse = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(bye.kind, "bye");
    }

    #[test]
    fn unknown_kind_returns_error_not_crash() {
        let mut state = WorkerState::new();
        let req = parse_request(r#"{"id":"k1","kind":"warp_drive"}"#).unwrap();
        match handle_request(&req, &mut state) {
            WorkerAction::Respond(resp) => {
                assert!(!resp.ok);
                assert!(resp.payload["message"].as_str().unwrap().contains("warp_drive"));
            }
            _ => panic!("未知类型应返回错误响应"),
        }
    }

    #[test]
    fn unconfigured_requests_fail_with_clear_error() {
        let mut state = WorkerState::new();
        for kind in ["plan", "run_task", "remember", "recall", "forget", "memory_stats"] {
            let req = parse_request(&format!(r#"{{"id":"x","kind":"{kind}"}}"#)).unwrap();
            match handle_request(&req, &mut state) {
                WorkerAction::Respond(resp) => {
                    assert!(!resp.ok, "{kind} 未配置时应失败");
                    let msg = resp.payload["message"].as_str().unwrap_or("");
                    assert!(msg.contains("configure"), "{kind} 错误应提示 configure: {msg}");
                }
                _ => panic!("{kind} 应返回 Respond"),
            }
        }
    }

    #[test]
    fn configure_rejects_bad_payload_and_agent() {
        let mut state = WorkerState::new();
        let tmp = tempfile::tempdir().unwrap();

        // 缺字段
        let req = parse_request(r#"{"id":"c1","kind":"configure","payload":{}}"#).unwrap();
        match handle_request(&req, &mut state) {
            WorkerAction::Respond(resp) => assert!(!resp.ok),
            _ => panic!("应返回 Respond"),
        }

        // 非法 agentId
        let payload = serde_json::json!({
            "baseUrl": "https://api.deepseek.com",
            "apiKey": "sk-test",
            "model": "deepseek-chat",
            "agentId": "hacker",
            "dataRoot": tmp.path().display().to_string(),
        });
        let req = WorkerRequest {
            id: "c2".to_string(),
            kind: "configure".to_string(),
            payload,
        };
        match handle_request(&req, &mut state) {
            WorkerAction::Respond(resp) => assert!(!resp.ok, "非法 agentId 应被拒绝"),
            _ => panic!("应返回 Respond"),
        }
        assert!(!state.is_configured());
    }

    #[test]
    fn configure_then_memory_flow_works() {
        let mut state = WorkerState::new();
        let tmp = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "baseUrl": "https://api.example.test",
            "apiKey": "sk-test",
            "model": "deepseek-chat",
            "agentId": "deepharness",
            "dataRoot": tmp.path().display().to_string(),
        });
        let req = WorkerRequest {
            id: "c1".to_string(),
            kind: "configure".to_string(),
            payload,
        };
        match handle_request(&req, &mut state) {
            WorkerAction::Respond(resp) => {
                assert!(resp.ok, "configure 失败: {:?}", resp.payload);
                assert_eq!(resp.kind, "configured");
            }
            _ => panic!("应返回 Respond"),
        }
        assert!(state.is_configured());

        // remember → recall → forget → stats
        let req = WorkerRequest {
            id: "m1".to_string(),
            kind: "remember".to_string(),
            payload: serde_json::json!({
                "kind": "fact",
                "content": "用户偏好中文回复",
                "tags": ["lang"],
                "importance": 0.8,
            }),
        };
        let resp = match handle_request(&req, &mut state) {
            WorkerAction::Respond(r) => r,
            _ => panic!("应返回 Respond"),
        };
        assert!(resp.ok, "remember 失败: {:?}", resp.payload);
        let id = resp.payload["id"].as_i64().unwrap();

        let req = WorkerRequest {
            id: "m2".to_string(),
            kind: "recall".to_string(),
            payload: serde_json::json!({ "query": "中文", "limit": 5 }),
        };
        let resp = match handle_request(&req, &mut state) {
            WorkerAction::Respond(r) => r,
            _ => panic!("应返回 Respond"),
        };
        assert!(resp.ok);
        assert_eq!(resp.payload.as_array().unwrap().len(), 1);

        let req = WorkerRequest {
            id: "m3".to_string(),
            kind: "forget".to_string(),
            payload: serde_json::json!({ "id": id }),
        };
        let resp = match handle_request(&req, &mut state) {
            WorkerAction::Respond(r) => r,
            _ => panic!("应返回 Respond"),
        };
        assert_eq!(resp.payload["existed"], true);

        let req = parse_request(r#"{"id":"m4","kind":"memory_stats"}"#).unwrap();
        let resp = match handle_request(&req, &mut state) {
            WorkerAction::Respond(r) => r,
            _ => panic!("应返回 Respond"),
        };
        assert_eq!(resp.payload["total"], 0);
    }
}

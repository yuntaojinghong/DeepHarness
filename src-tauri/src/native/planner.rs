//! 多步规划器。
//!
//! 把用户的自然语言目标转换为结构化 JSON 计划（`Plan`）。规划器对
//! 模型的输出做严格校验：非法 JSON、缺字段、步骤超限都会触发一次
//! "带错误反馈的重试"；重试仍失败则返回明确错误。
//!
//! 计划 JSON 约定（system prompt 中下发给模型）：
//!
//! ```json
//! {
//!   "goal": "…",
//!   "steps": [
//!     { "title": "…", "tool": "read_text_file|null",
//!       "args": { "path": "…" }, "expected": "…" }
//!   ]
//! }
//! ```
//!
//! `tool: null` 表示纯回答步骤（如最终总结），执行器直接把
//! `expected` 当作说明展示，不调用工具。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::native::model::{ChatMessage, ModelProvider};
use crate::native::tools::TOOLS;

/// 单个计划步骤。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub title: String,
    /// 工具名；`None` 表示纯回答/总结步骤。
    pub tool: Option<String>,
    /// 工具参数（工具步骤必填，回答步骤可为 Null）。
    pub args: serde_json::Value,
    /// 预期结果（回答步骤时作为说明文本）。
    pub expected: String,
}

/// 完整计划。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

/// 计划步骤数上限：防止模型输出失控的长计划。
pub const MAX_STEPS: usize = 16;

pub struct Planner {
    provider: std::sync::Arc<dyn ModelProvider>,
}

impl Planner {
    pub fn new(provider: std::sync::Arc<dyn ModelProvider>) -> Self {
        Self { provider }
    }

    /// 生成计划。
    ///
    /// `context` 为已召回的相关记忆 / 会话摘要等补充材料（可空）。
    pub fn make_plan(&self, goal: &str, context: &str) -> AppResult<Plan> {
        if goal.trim().is_empty() {
            return Err(AppError::Other("目标不能为空".to_string()));
        }

        let tools_desc = TOOLS
            .iter()
            .map(|t| format!("- {}: {} 参数: {}", t.name, t.description, t.args_schema))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(
            "你是 DeepHarness 桌面 Agent 的规划器。把用户目标拆解为不超过 {MAX_STEPS} 个可执行步骤。\n\
             可用工具：\n{tools_desc}\n\
             只输出一个 JSON 对象，格式：\n\
             {{\"goal\": string, \"steps\": [{{\"title\": string, \"tool\": string|null, \"args\": object|null, \"expected\": string}}]}}\n\
             规则：\n\
             1. 每个工具步骤给出具体的 args；tool 为 null 的步骤是纯说明/总结步骤，expected 写给用户看的话。\n\
             2. 不要编造不存在的工具；路径必须是绝对路径。\n\
             3. 只输出 JSON，不要输出其它文字或代码围栏。"
        );

        let user_msg = if context.trim().is_empty() {
            format!("目标：{goal}")
        } else {
            format!("目标：{goal}\n\n相关上下文：\n{context}")
        };

        let messages = [
            ChatMessage::system(system),
            ChatMessage::user(user_msg),
        ];

        // 首次尝试
        let first = self.provider.chat(&messages, 0.2)?;
        match parse_plan(&first) {
            Ok(plan) => Ok(plan),
            Err(first_err) => {
                // 带错误反馈重试一次
                tracing::warn!(error = %first_err, "计划解析失败，重试一次");
                let mut retry_messages = messages.to_vec();
                retry_messages.push(ChatMessage::assistant(&first));
                retry_messages.push(ChatMessage::user(format!(
                    "上面的输出无法解析为计划 JSON：{first_err}\n请严格按约定格式重新只输出一个 JSON 对象。"
                )));
                let second = self.provider.chat(&retry_messages, 0.1)?;
                parse_plan(&second).map_err(|e| {
                    AppError::Other(format!("计划生成失败（重试后仍无效）: {e}"))
                })
            }
        }
    }

    /// 依据失败原因与反思建议修订单个步骤（执行器重试前调用）。
    ///
    /// 返回 `Some(修订步骤)` 表示模型给出了可用的修正；模型不可用或
    /// 输出无效时返回 `None`，调用方按原步骤重试——保证"重试一定会
    /// 发生"这一确定性不被模型故障破坏。
    pub fn revise_step(
        &self,
        goal: &str,
        step: &PlanStep,
        failure: &str,
        advice: &str,
    ) -> Option<PlanStep> {
        let tools_desc = TOOLS
            .iter()
            .map(|t| format!("- {}: {} 参数: {}", t.name, t.description, t.args_schema))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(
            "你是 DeepHarness 桌面 Agent 的规划器。某一步执行失败，请修订该步骤使其可执行成功。\n\
             可用工具：\n{tools_desc}\n\
             只输出一个 JSON 对象，格式：\n\
             {{\"title\": string, \"tool\": string|null, \"args\": object|null, \"expected\": string}}\n\
             规则：\n\
             1. 只修订这一个步骤，不要输出整个计划。\n\
             2. 失败源于参数错误时修正 args（例如改用正确的绝对路径）；该步骤确实无法完成时，\n\
                可降级为 tool 为 null 的说明步骤，并在 expected 中说明原因。\n\
             3. 不要编造不存在的工具；路径必须是绝对路径。\n\
             4. 只输出 JSON，不要输出其它文字或代码围栏。"
        );

        let user_msg = format!(
            "目标：{goal}\n原步骤：{}\n失败原因：{}\n反思建议：{}",
            serde_json::to_string(step).unwrap_or_else(|_| "{}".to_string()),
            if failure.trim().is_empty() { "（无）" } else { failure },
            if advice.trim().is_empty() { "（无）" } else { advice },
        );

        let messages = [
            ChatMessage::system(system),
            ChatMessage::user(user_msg),
        ];

        let raw = match self.provider.chat(&messages, 0.1) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "步骤修订调用失败，按原步骤重试");
                return None;
            }
        };
        match parse_step_json(&raw) {
            Ok(step) => Some(step),
            Err(e) => {
                tracing::warn!(error = %e, "步骤修订输出无效，按原步骤重试");
                None
            }
        }
    }
}

/// 解析"单个步骤 JSON"（reviser 的输出）：剥离围栏后按步骤规则校验。
pub fn parse_step_json(raw: &str) -> AppResult<PlanStep> {
    let json_text = extract_json(raw)?;
    let value: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|e| AppError::Other(format!("修订步骤不是合法 JSON: {e}")))?;
    parse_step_value(&value, 0)
}

/// 校验并转换单个步骤的 JSON 值（`index` 仅用于错误信息，从 0 起）。
fn parse_step_value(sv: &serde_json::Value, index: usize) -> AppResult<PlanStep> {
    let title = sv
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Other(format!("第 {} 步缺少 title", index + 1)))?;
    let tool = match sv.get("tool") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(name)) => {
            let name = name.trim().to_string();
            if name.is_empty() {
                None
            } else {
                // 工具必须在注册表中，防止模型发明工具
                if crate::native::tools::find_tool(&name).is_none() {
                    return Err(AppError::Other(format!(
                        "第 {} 步引用了未知工具: {name}",
                        index + 1
                    )));
                }
                Some(name)
            }
        }
        Some(_) => {
            return Err(AppError::Other(format!(
                "第 {} 步的 tool 必须是字符串或 null",
                index + 1
            )));
        }
    };
    let args = sv.get("args").cloned().unwrap_or(serde_json::Value::Null);
    let expected = sv
        .get("expected")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if tool.is_some() && args.is_null() {
        return Err(AppError::Other(format!(
            "第 {} 步是工具步骤但缺少 args",
            index + 1
        )));
    }
    Ok(PlanStep { title, tool, args, expected })
}

/// 解析并校验模型输出的计划 JSON。
///
/// 宽容处理常见问题：剥离 ``` 代码围栏、忽略 JSON 前后的说明文字。
pub fn parse_plan(raw: &str) -> AppResult<Plan> {
    let json_text = extract_json(raw)?;
    let value: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|e| AppError::Other(format!("计划不是合法 JSON: {e}")))?;

    let goal = value
        .get("goal")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Other("计划缺少 goal 字段".to_string()))?;

    let steps_value = value
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Other("计划缺少 steps 数组".to_string()))?;
    if steps_value.is_empty() {
        return Err(AppError::Other("计划的 steps 不能为空".to_string()));
    }
    if steps_value.len() > MAX_STEPS {
        return Err(AppError::Other(format!(
            "计划步骤数 {} 超过上限 {MAX_STEPS}",
            steps_value.len()
        )));
    }

    let mut steps = Vec::with_capacity(steps_value.len());
    for (i, sv) in steps_value.iter().enumerate() {
        steps.push(parse_step_value(sv, i)?);
    }

    Ok(Plan { goal, steps })
}

/// 从模型输出中提取 JSON 文本：剥离代码围栏与前后缀说明。
fn extract_json(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    // ```json ... ``` 围栏
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.trim_start_matches("json").trim_start();
        if let Some(end) = rest.rfind("```") {
            return Ok(rest[..end].trim().to_string());
        }
    }
    // 裸 JSON：找第一个 { 与最后一个 }
    let start = trimmed.find('{').ok_or_else(|| {
        AppError::Other("输出中找不到 JSON 对象（缺 '{'）".to_string())
    })?;
    let end = trimmed.rfind('}').ok_or_else(|| {
        AppError::Other("输出中找不到 JSON 对象（缺 '}'）".to_string())
    })?;
    if end < start {
        return Err(AppError::Other("输出中的大括号不匹配".to_string()));
    }
    Ok(trimmed[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 固定脚本的假模型：按调用次数依次返回预置回复。
    struct FakeProvider {
        replies: std::sync::Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    impl FakeProvider {
        fn new(replies: Vec<String>) -> Self {
            Self { replies: std::sync::Mutex::new(replies), calls: AtomicUsize::new(0) }
        }
    }

    impl ModelProvider for FakeProvider {
        fn chat(&self, _messages: &[ChatMessage], _t: f64) -> AppResult<String> {
            let mut q = self.replies.lock().unwrap();
            self.calls.fetch_add(1, Ordering::SeqCst);
            if q.is_empty() {
                Err(AppError::Other("脚本耗尽".to_string()))
            } else {
                Ok(q.remove(0))
            }
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    fn good_plan_json() -> String {
        serde_json::json!({
            "goal": "整理工作区",
            "steps": [
                {"title": "看看有什么", "tool": "list_directory",
                 "args": {"path": "C:/ws"}, "expected": "目录清单"},
                {"title": "总结", "tool": null, "args": null, "expected": "一句话总结"}
            ]
        })
        .to_string()
    }

    #[test]
    fn parses_clean_plan() {
        let p = Planner::new(std::sync::Arc::new(FakeProvider::new(vec![good_plan_json()])));
        let plan = p.make_plan("整理", "").unwrap();
        assert_eq!(plan.goal, "整理工作区");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].tool.as_deref(), Some("list_directory"));
        assert_eq!(plan.steps[1].tool, None);
        assert_eq!(plan.steps[1].expected, "一句话总结");
    }

    #[test]
    fn strips_code_fence_and_prose() {
        let raw = format!("好的，这是计划：\n```json\n{}\n```\n请查收。", good_plan_json());
        let plan = parse_plan(&raw).unwrap();
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn retries_once_with_feedback_then_succeeds() {
        let provider = FakeProvider::new(vec![
            "这不是 JSON".to_string(),
            good_plan_json(),
        ]);
        let p = Planner::new(std::sync::Arc::new(provider));
        let plan = p.make_plan("g", "").unwrap();
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn fails_after_retry_exhausted() {
        let provider = FakeProvider::new(vec![
            "坏输出 1".to_string(),
            "坏输出 2".to_string(),
        ]);
        let p = Planner::new(std::sync::Arc::new(provider));
        assert!(p.make_plan("g", "").is_err());
    }

    #[test]
    fn rejects_unknown_tools_and_overlong_plans() {
        let mut steps = Vec::new();
        for i in 0..(MAX_STEPS + 1) {
            steps.push(serde_json::json!({"title": format!("s{i}"), "tool": null, "expected": "x"}));
        }
        let raw = serde_json::json!({"goal": "g", "steps": steps}).to_string();
        let err = parse_plan(&raw).unwrap_err();
        assert!(err.to_string().contains("超过上限"));

        let unknown_tool = serde_json::json!({
            "goal": "g",
            "steps": [{"title": "s", "tool": "warp_drive", "args": {}}]
        })
        .to_string();
        assert!(parse_plan(&unknown_tool).is_err());
    }

    #[test]
    fn tool_step_without_args_rejected() {
        let raw = serde_json::json!({
            "goal": "g",
            "steps": [{"title": "s", "tool": "read_text_file"}]
        })
        .to_string();
        let err = parse_plan(&raw).unwrap_err();
        assert!(err.to_string().contains("缺少 args"));
    }

    #[test]
    fn empty_goal_rejected_before_calling_model() {
        let p = Planner::new(std::sync::Arc::new(FakeProvider::new(vec![])));
        assert!(p.make_plan("   ", "").is_err());
    }

    fn sample_step() -> PlanStep {
        PlanStep {
            title: "读文件".to_string(),
            tool: Some("read_text_file".to_string()),
            args: serde_json::json!({"path": "C:/ws/missing.txt"}),
            expected: "文件内容".to_string(),
        }
    }

    #[test]
    fn revise_step_returns_corrected_step() {
        let revised = serde_json::json!({
            "title": "写新文件", "tool": "write_text_file",
            "args": {"path": "C:/ws/new.txt", "contents": "hi"},
            "expected": "新文件已创建"
        })
        .to_string();
        let p = Planner::new(std::sync::Arc::new(FakeProvider::new(vec![revised])));
        let out = p
            .revise_step("整理工作区", &sample_step(), "路径不存在", "改为写入新文件")
            .expect("应给出修订步骤");
        assert_eq!(out.title, "写新文件");
        assert_eq!(out.tool.as_deref(), Some("write_text_file"));
        assert_eq!(out.args["path"], "C:/ws/new.txt");
    }

    #[test]
    fn revise_step_degrades_to_none_on_failure() {
        // 模型调用直接报错（脚本为空）→ None，调用方按原步骤重试
        let p = Planner::new(std::sync::Arc::new(FakeProvider::new(vec![])));
        assert!(p
            .revise_step("g", &sample_step(), "boom", "advice")
            .is_none());
    }

    #[test]
    fn revise_step_rejects_invalid_output() {
        // 输出引用了不存在的工具 → None（不污染执行流程）
        let bad = serde_json::json!({
            "title": "乱来", "tool": "warp_drive", "args": {}, "expected": "x"
        })
        .to_string();
        let p = Planner::new(std::sync::Arc::new(FakeProvider::new(vec![bad])));
        assert!(p.revise_step("g", &sample_step(), "boom", "").is_none());
    }

    #[test]
    fn parse_step_json_strips_fence_and_validates() {
        let raw = format!(
            "修订如下：\n```json\n{}\n```",
            serde_json::json!({"title": "t", "tool": null, "expected": "说明"})
        );
        let step = parse_step_json(&raw).unwrap();
        assert_eq!(step.tool, None);
        assert_eq!(step.expected, "说明");
        assert!(parse_step_json("完全不是 JSON").is_err());
    }
}

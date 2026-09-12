//! 反思与纠错。
//!
//! 每个工具步骤执行完毕后，由 `Reflector` 让模型对"步骤预期 vs 实际
//! 结果"做结构化评审：
//!
//! ```json
//! { "success": bool, "summary": string, "shouldRetry": bool, "advice": string }
//! ```
//!
//! 评审失败（模型不可用 / 输出非法）不阻塞任务：`review_step` 会退化为
//! "按工具层错误判断成败"的保守策略，保证任务循环永远有确定性出口。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::native::model::{ChatMessage, ModelProvider};
use crate::native::planner::PlanStep;

/// 一步执行后的评审结论。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepReview {
    pub success: bool,
    pub summary: String,
    /// 失败且值得原样重试（例如瞬时 IO 错误）时为 true。
    pub should_retry: bool,
    /// 给规划器/用户的修正建议。
    pub advice: String,
}

pub struct Reflector {
    provider: std::sync::Arc<dyn ModelProvider>,
}

impl Reflector {
    pub fn new(provider: std::sync::Arc<dyn ModelProvider>) -> Self {
        Self { provider }
    }

    /// 评审一步的执行结果。
    ///
    /// `result_json` 为工具执行结果（成功）或错误信息（失败）。
    /// `tool_error` 指示执行层是否已经报错。
    pub fn review_step(
        &self,
        goal: &str,
        step: &PlanStep,
        result_json: &str,
        tool_error: bool,
    ) -> AppResult<StepReview> {
        // 保守基线：评审模型不可用时按执行层结果判定
        let fallback = |summary: String| StepReview {
            success: !tool_error,
            summary,
            should_retry: false,
            advice: String::new(),
        };

        let system = "你是 DeepHarness Agent 的评审器。根据工具执行结果判断该步骤是否达成预期。\
 只输出一个 JSON 对象：\
 {\"success\": bool, \"summary\": string, \"shouldRetry\": bool, \"advice\": string}\
 。success 表示结果是否达到步骤预期；shouldRetry 仅在失败且看起来是暂时性错误（如网络抖动、文件被占用）时为 true；advice 写给规划器的下一步建议；只输出 JSON。"
            .to_string();

        let user_msg = format!(
            "总目标：{goal}\n步骤：{title}\n预期：{expected}\n工具：{tool}\n执行结果：\n{result}",
            title = step.title,
            expected = step.expected,
            tool = step.tool.as_deref().unwrap_or("(纯回答)"),
            result = result_json,
        );

        let messages = [ChatMessage::system(system), ChatMessage::user(user_msg)];

        match self.provider.chat(&messages, 0.1) {
            Ok(raw) => parse_review(&raw).or_else(|e| {
                tracing::warn!(error = %e, "评审输出解析失败，使用保守判定");
                Ok(fallback(format!("评审输出无法解析，按执行层判定: {raw}")))
            }),
            Err(e) => {
                tracing::warn!(error = %e, "评审模型调用失败，使用保守判定");
                Ok(fallback(format!("评审模型不可用: {e}")))
            }
        }
    }
}

/// 解析评审 JSON（宽容：剥离围栏与前后缀）。
pub fn parse_review(raw: &str) -> AppResult<StepReview> {
    let text = strip_fences(raw);
    let start = text.find('{').ok_or_else(|| {
        AppError::Other("评审输出缺少 JSON 对象".to_string())
    })?;
    let end = text.rfind('}').ok_or_else(|| {
        AppError::Other("评审输出缺少 JSON 对象".to_string())
    })?;
    if end < start {
        return Err(AppError::Other("评审输出大括号不匹配".to_string()));
    }
    let value: serde_json::Value = serde_json::from_str(&text[start..=end])
        .map_err(|e| AppError::Other(format!("评审 JSON 解析失败: {e}")))?;
    let success = value
        .get("success")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| AppError::Other("评审缺少 success 布尔字段".to_string()))?;
    let summary = value
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let should_retry = value
        .get("shouldRetry")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let advice = value
        .get("advice")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(StepReview { success, summary, should_retry, advice })
}

fn strip_fences(raw: &str) -> &str {
    let t = raw.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.trim_start_matches("json").trim_start();
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim();
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProvider {
        replies: std::sync::Mutex<Vec<Option<String>>>,
        calls: AtomicUsize,
    }

    impl FakeProvider {
        fn new(replies: Vec<Option<String>>) -> Self {
            Self { replies: std::sync::Mutex::new(replies), calls: AtomicUsize::new(0) }
        }
    }

    impl ModelProvider for FakeProvider {
        fn chat(&self, _m: &[ChatMessage], _t: f64) -> AppResult<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut q = self.replies.lock().unwrap();
            match q.is_empty() {
                true => Err(AppError::Other("脚本耗尽".to_string())),
                false => Ok(q.remove(0).unwrap_or_default()),
            }
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    fn step() -> PlanStep {
        PlanStep {
            title: "读文件".to_string(),
            tool: Some("read_text_file".to_string()),
            args: serde_json::json!({"path": "C:/a.txt"}),
            expected: "文件内容".to_string(),
        }
    }

    #[test]
    fn parses_clean_review() {
        let raw = serde_json::json!({
            "success": true, "summary": "读到了内容", "shouldRetry": false, "advice": ""
        })
        .to_string();
        let r = parse_review(&raw).unwrap();
        assert!(r.success);
        assert_eq!(r.summary, "读到了内容");
        assert!(!r.should_retry);
    }

    #[test]
    fn strips_fence() {
        let raw = format!("```json\n{}\n```",
            serde_json::json!({"success": false, "summary": "s", "shouldRetry": true, "advice": "换路径"}));
        let r = parse_review(&raw).unwrap();
        assert!(!r.success);
        assert!(r.should_retry);
        assert_eq!(r.advice, "换路径");
    }

    #[test]
    fn model_unavailable_falls_back_conservatively() {
        let refl = Reflector::new(std::sync::Arc::new(FakeProvider::new(vec![])));
        let r = refl.review_step("g", &step(), "{}", true).unwrap();
        assert!(!r.success, "执行层失败时保守判定为失败");
        let r = refl.review_step("g", &step(), "{}", false).unwrap();
        assert!(r.success, "执行层成功时保守判定为成功");
    }

    #[test]
    fn malformed_review_falls_back() {
        let refl = Reflector::new(std::sync::Arc::new(FakeProvider::new(vec![
            Some("垃圾输出".to_string()),
        ])));
        let r = refl.review_step("g", &step(), "{}", true).unwrap();
        assert!(!r.success);
        assert!(r.summary.contains("无法解析"));
    }

    #[test]
    fn missing_success_field_falls_back() {
        let refl = Reflector::new(std::sync::Arc::new(FakeProvider::new(vec![
            Some(r#"{"summary":"只有summary"}"#.to_string()),
        ])));
        let r = refl.review_step("g", &step(), "{}", false).unwrap();
        assert!(r.success);
        assert!(r.summary.contains("无法解析"));
    }
}

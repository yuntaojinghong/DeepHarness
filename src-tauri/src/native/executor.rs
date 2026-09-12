//! 任务执行编排：规划 → 逐步执行 → 反思纠错 → 记忆沉淀。
//!
//! `TaskRunner` 是自研 Agent 的主循环：
//!
//! ```text
//! goal ──► Planner.make_plan ──► 逐个 PlanStep：
//!             ├─ 工具步骤：execute_tool（白名单校验）→ Reflector.review_step
//!             │            失败且 shouldRetry → 重试一次
//!             └─ 回答步骤：expected 直接作为该步输出
//!          ──► 总结（模型生成，失败时用步骤摘要拼接兜底）
//!          ──► MemoryStore 记录 event（任务完成/失败）
//! ```
//!
//! 所有模型调用失败都不 panic：每一步都有明确的降级路径。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::native::memory::MemoryStore;
use crate::native::model::{ChatMessage, ModelProvider};
use crate::native::planner::{Plan, PlanStep, Planner};
use crate::native::reflector::{Reflector, StepReview};
use crate::native::tools::{execute_tool, ToolContext};

/// 单步执行记录（返回给前端 / 会话存储）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub title: String,
    pub tool: Option<String>,
    pub ok: bool,
    /// 工具原始结果（JSON 字符串）或错误信息。
    pub result: String,
    pub review: StepReview,
    /// 是否经历了一次重试。
    pub retried: bool,
    /// 重试是否使用了规划器修订后的步骤（false 表示按原步骤重试）。
    pub revised: bool,
}

/// 任务执行结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskOutcome {
    pub goal: String,
    pub success: bool,
    pub summary: String,
    pub steps: Vec<StepRecord>,
}

/// 任务执行器。
pub struct TaskRunner<'a> {
    planner: Planner,
    reflector: Reflector,
    summarizer: Arc<dyn ModelProvider>,
    ctx: ToolContext<'a>,
    memory: &'a MemoryStore,
    /// 每步重试上限（当前固定 1 次）。
    max_retries: usize,
}

impl<'a> TaskRunner<'a> {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        ctx: ToolContext<'a>,
        memory: &'a MemoryStore,
    ) -> Self {
        // 规划器与执行器共用同一份工具目录：内置工具 + 本次任务可见的
        // 插件工具。少了它，模型既规划不出插件步骤，修订时也会把插件
        // 步骤当成"未知工具"而错误降级。
        let catalog = tool_catalog(ctx.plugins);
        Self {
            planner: Planner::with_catalog(provider.clone(), catalog),
            reflector: Reflector::new(provider.clone()),
            summarizer: provider,
            ctx,
            memory,
            max_retries: 1,
        }
    }

    /// 执行完整任务。
    pub fn run(&self, goal: &str, context: &str) -> AppResult<TaskOutcome> {
        let plan = self.planner.make_plan(goal, context)?;
        self.remember_event("event", &format!("开始任务: {goal}"), 0.3)?;

        let mut records: Vec<StepRecord> = Vec::new();
        let mut any_failure = false;

        for step in &plan.steps {
            let record = self.run_step(goal, step);
            if !record.ok {
                any_failure = true;
            }
            records.push(record);
        }

        let summary = self.summarize(&plan, &records);
        let outcome = TaskOutcome {
            goal: plan.goal.clone(),
            success: !any_failure,
            summary,
            steps: records,
        };

        let event = if outcome.success {
            format!("任务完成: {goal} — {}", outcome.summary)
        } else {
            format!("任务失败: {goal} — {}", outcome.summary)
        };
        let _ = self.remember_event("event", &event, 0.4);
        Ok(outcome)
    }

    fn run_step(&self, goal: &str, step: &PlanStep) -> StepRecord {
        let mut attempt = 0usize;
        let mut retried = false;
        let mut revised = false;
        // 当前生效的步骤：重试时可能被规划器修订后替换
        let mut current = step.clone();
        loop {
            let (result, tool_error) = match &current.tool {
                None => {
                    // 纯回答步骤：expected 即输出
                    (current.expected.clone(), false)
                }
                Some(tool_name) => {
                    let args = if current.args.is_null() {
                        serde_json::json!({})
                    } else {
                        current.args.clone()
                    };
                    match execute_tool(&self.ctx, tool_name, &args) {
                        Ok(v) => (v.to_string(), false),
                        Err(e) => (e.to_string(), true),
                    }
                }
            };

            let review = self
                .reflector
                .review_step(goal, &current, &result, tool_error)
                .unwrap_or_else(|_| StepReview {
                    success: !tool_error,
                    summary: "评审不可用，按执行层判定".to_string(),
                    should_retry: false,
                    advice: String::new(),
                });

            if review.success || attempt >= self.max_retries || !review.should_retry {
                return StepRecord {
                    title: current.title.clone(),
                    tool: current.tool.clone(),
                    ok: review.success,
                    result,
                    review,
                    retried,
                    revised,
                };
            }

            // 失败且评审建议重试：先请规划器依据失败原因与建议修订单步，
            // 修订不可用时按原步骤重试（重试必定发生）。
            attempt += 1;
            retried = true;
            let failure = result.clone();
            match self
                .planner
                .revise_step(goal, &current, &failure, &review.advice)
            {
                Some(next) => {
                    tracing::info!(
                        from = %current.title,
                        to = %next.title,
                        "步骤已按反思建议修订，重试使用修订后的步骤"
                    );
                    current = next;
                    revised = true;
                }
                None => {
                    // 保持 current 不变，按原步骤重试
                }
            }
            let _ = self.remember_event(
                "reflection",
                &format!(
                    "步骤「{}」失败将重试（{}）: {}",
                    step.title,
                    if revised { "已修订" } else { "原步骤" },
                    review.summary
                ),
                0.2,
            );
        }
    }

    /// 生成最终总结；模型失败时用步骤摘要拼接兜底。
    fn summarize(&self, plan: &Plan, records: &[StepRecord]) -> String {
        let steps_brief = records
            .iter()
            .map(|r| {
                format!(
                    "- {} [{}]: {}",
                    r.title,
                    if r.ok { "成功" } else { "失败" },
                    r.review.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let messages = [
            ChatMessage::system(
                "你是 DeepHarness Agent。根据目标与各步骤结果，用不超过 200 字向用户总结任务执行情况。只输出总结正文。",
            ),
            ChatMessage::user(format!(
                "目标：{}\n步骤结果：\n{steps_brief}",
                plan.goal
            )),
        ];
        self.summarizer
            .chat(&messages, 0.3)
            .unwrap_or_else(|_| format!("共 {} 步，{}。", records.len(), steps_brief))
    }

    fn remember_event(&self, kind: &str, content: &str, importance: f64) -> AppResult<i64> {
        self.memory.remember(kind, content, &["task"], importance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::memory::MemoryStore;
    use crate::native::model::ChatMessage;
    use crate::paths::AgentDirs;
    use crate::permissions::PermissionStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 按调用序号取回复的假模型。
    /// 调用顺序（完整 run 流程）：plan → (每工具步 review ×N) → … → summarize。
    struct ScriptedProvider {
        replies: std::sync::Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    impl ScriptedProvider {
        fn new(replies: Vec<String>) -> Self {
            Self { replies: std::sync::Mutex::new(replies), calls: AtomicUsize::new(0) }
        }
    }

    impl ModelProvider for ScriptedProvider {
        fn chat(&self, _m: &[ChatMessage], _t: f64) -> AppResult<String> {
            let mut q = self.replies.lock().unwrap();
            self.calls.fetch_add(1, Ordering::SeqCst);
            if q.is_empty() {
                // 脚本耗尽 = 模拟模型不可用（供兜底路径测试使用）
                return Err(crate::error::AppError::Other("脚本耗尽".to_string()));
            }
            Ok(q.remove(0))
        }
        fn name(&self) -> &str {
            "scripted"
        }
    }

    fn setup(tag: &str) -> (tempfile::TempDir, AgentDirs, PermissionStore, MemoryStore) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(tag);
        let dirs = AgentDirs::from_root(root.clone(), "deepharness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let perms = PermissionStore::new(root.join("permissions.json"));
        let memory = MemoryStore::open(&root.join("config").join("memory.db")).unwrap();
        (tmp, dirs, perms, memory)
    }

    fn plan_json(dirs: &AgentDirs) -> String {
        serde_json::json!({
            "goal": "在工作区写一个清单",
            "steps": [
                {"title": "写文件", "tool": "write_text_file",
                 "args": {"path": dirs.workspace.join("todo.txt").display().to_string(),
                          "contents": "任务一"},
                 "expected": "文件写入成功"},
                {"title": "总结", "tool": null, "args": null, "expected": "已完成"}
            ]
        })
        .to_string()
    }

    #[test]
    fn full_run_happy_path() {
        let (_t, dirs, perms, memory) = setup("happy");
        let file = dirs.workspace.join("todo.txt");
        let plan = plan_json(&dirs);
        let review_ok = serde_json::json!({
            "success": true, "summary": "写入成功", "shouldRetry": false, "advice": ""
        })
        .to_string();
        // 顺序：plan → review(工具步) → summarize
        let provider = Arc::new(ScriptedProvider::new(vec![
            plan,
            review_ok,
            "任务顺利完成，文件已写入。".to_string(),
        ]));
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let runner = TaskRunner::new(provider, ctx, &memory);
        let outcome = runner.run("在工作区写一个清单", "").unwrap();

        assert!(outcome.success);
        assert_eq!(outcome.steps.len(), 2);
        assert!(outcome.steps[0].ok);
        assert_eq!(outcome.steps[1].tool, None);
        assert!(outcome.summary.contains("顺利"));
        assert!(file.exists());
        // 任务开始/完成事件已入记忆
        assert!(memory.stats().unwrap().total >= 2);
    }

    #[test]
    fn failing_step_marks_outcome_unsuccessful() {
        let (_t, dirs, perms, memory) = setup("fail");
        let plan = serde_json::json!({
            "goal": "读不存在的文件",
            "steps": [
                {"title": "读文件", "tool": "read_text_file",
                 "args": {"path": dirs.workspace.join("ghost.txt").display().to_string()},
                 "expected": "有内容"},
                {"title": "总结", "tool": null, "expected": "x"}
            ]
        })
        .to_string();
        let review_fail = serde_json::json!({
            "success": false, "summary": "文件不存在", "shouldRetry": false, "advice": "先确认路径"
        })
        .to_string();
        let provider = Arc::new(ScriptedProvider::new(vec![
            plan,
            review_fail,
            "部分步骤失败。".to_string(),
        ]));
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let runner = TaskRunner::new(provider, ctx, &memory);
        let outcome = runner.run("读不存在的文件", "").unwrap();

        assert!(!outcome.success);
        assert!(!outcome.steps[0].ok);
        assert!(outcome.steps[0].result.contains("不存在"));
    }

    #[test]
    fn retry_once_when_review_suggests() {
        let (_t, dirs, perms, memory) = setup("retry");
        let file = dirs.workspace.join("later.txt");
        // 第一次工具执行时文件不存在 → 报错；评审建议重试；
        // 重试前规划器给出修订步骤（降级为说明步骤）→ 第二次执行仍被判失败。
        // 断言：恰好 5 次模型调用（plan / review / revise / review / summarize），
        // 且重试使用了修订后的步骤。
        let plan = serde_json::json!({
            "goal": "读文件",
            "steps": [
                {"title": "读文件", "tool": "read_text_file",
                 "args": {"path": file.display().to_string()}, "expected": "内容"}
            ]
        })
        .to_string();
        let review_retry = serde_json::json!({
            "success": false, "summary": "暂时不可用", "shouldRetry": true, "advice": "换一种方式"
        })
        .to_string();
        let revised_step = serde_json::json!({
            "title": "说明文件缺失", "tool": null, "args": null,
            "expected": "目标文件不存在，建议先创建它"
        })
        .to_string();
        let review_fail = serde_json::json!({
            "success": false, "summary": "仍失败", "shouldRetry": false, "advice": ""
        })
        .to_string();
        let provider = Arc::new(ScriptedProvider::new(vec![
            plan,
            review_retry,
            revised_step,
            review_fail,
            "最终失败。".to_string(),
        ]));
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let runner = TaskRunner::new(provider.clone(), ctx, &memory);
        let outcome = runner.run("读文件", "").unwrap();
        assert!(!outcome.success);
        assert!(outcome.steps[0].retried, "应发生一次重试");
        assert!(outcome.steps[0].revised, "重试应使用修订后的步骤");
        assert_eq!(outcome.steps[0].title, "说明文件缺失", "记录应反映修订后的步骤");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn retry_falls_back_to_original_step_when_revision_invalid() {
        let (_t, dirs, perms, memory) = setup("retry_fallback");
        let file = dirs.workspace.join("never.txt");
        let plan = serde_json::json!({
            "goal": "读文件",
            "steps": [
                {"title": "读文件", "tool": "read_text_file",
                 "args": {"path": file.display().to_string()}, "expected": "内容"}
            ]
        })
        .to_string();
        let review_retry = serde_json::json!({
            "success": false, "summary": "暂时不可用", "shouldRetry": true, "advice": "重试"
        })
        .to_string();
        let review_fail = serde_json::json!({
            "success": false, "summary": "仍失败", "shouldRetry": false, "advice": ""
        })
        .to_string();
        // 第 3 次调用是修订请求，故意给不可解析的输出 → 回退为原步骤重试
        let provider = Arc::new(ScriptedProvider::new(vec![
            plan,
            review_retry,
            "这不是 JSON".to_string(),
            review_fail,
            "最终失败。".to_string(),
        ]));
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let runner = TaskRunner::new(provider.clone(), ctx, &memory);
        let outcome = runner.run("读文件", "").unwrap();
        assert!(!outcome.success);
        assert!(outcome.steps[0].retried);
        assert!(!outcome.steps[0].revised, "修订无效时应按原步骤重试");
        assert_eq!(outcome.steps[0].title, "读文件");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn summarize_falls_back_to_brief_when_model_fails() {
        let (_t, dirs, perms, memory) = setup("fallback");
        let plan = serde_json::json!({
            "goal": "纯说明任务",
            "steps": [{"title": "说明", "tool": null, "expected": "这是一个回答步骤"}]
        })
        .to_string();
        // 只给 plan 的回复：总结那次调用脚本耗尽 → 模型不可用 → 走兜底拼接
        let provider = Arc::new(ScriptedProvider::new(vec![plan]));
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let runner = TaskRunner::new(provider, ctx, &memory);
        let outcome = runner.run("纯说明任务", "").unwrap();
        assert!(outcome.success);
        // 兜底文案形如「共 1 步，…」
        assert!(outcome.summary.contains("共 1 步"), "应走兜底拼接: {}", outcome.summary);
    }

    /// 插件工具必须进入执行器的规划器 —— 否则模型规划不出插件步骤，
    /// 失败修订时还会把插件步骤判成"未知工具"而错误降级。
    #[test]
    fn runner_planner_catalog_includes_plugin_tools() {
        use crate::plugins::{ManifestTool, PluginHost, PluginRuntime, PluginTool};
        use std::path::PathBuf;

        let (_t, dirs, perms, memory) = setup("plugcatalog");
        let runtime = PluginRuntime::for_test(
            PluginHost::new(PathBuf::from("node"), PathBuf::from("host.mjs")),
            vec![PluginTool {
                tool: ManifestTool {
                    name: "demo_echo".to_string(),
                    description: "回声".to_string(),
                    args_schema: r#"{ "text": string }"#.to_string(),
                },
                plugin_id: "com.test.demo".to_string(),
                plugin_name: "Demo".to_string(),
                plugin_dir: PathBuf::from("C:/plugins/demo"),
                entry: "index.js".to_string(),
            }],
        );
        let ctx = ToolContext { perms: &perms, dirs: &dirs, plugins: Some(&runtime) };
        let provider = Arc::new(ScriptedProvider::new(vec![]));
        let runner = TaskRunner::new(provider, ctx, &memory);

        let names: Vec<&str> = runner.planner.catalog().iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"demo_echo"), "规划器应看到插件工具: {names:?}");
        assert!(names.contains(&"read_text_file"), "内置工具不能丢: {names:?}");
    }
}

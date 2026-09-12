//! 跨模块公开 API 的集成测试。
//!
//! 与各模块内部的 `#[cfg(test)]` 单元测试互补：这里只通过 crate 的公开
//! 接口（`deepharness_lib::…`）访问，验证模块之间的契约在对外暴露后依然
//! 成立（例如「规划器只接受注册表里存在的工具」这类跨模块约束）。
//!
//! 附带作用：它是本包唯一的 test 类型目标，`build.rs` 依赖它才能用
//! `cargo:rustc-link-arg-tests` 把窗口清单**只**注入测试可执行文件，
//! 而不影响 bin 目标由 tauri-build 嵌入的原生清单。

use deepharness_lib::agents::agent_config::AgentModelConfig;
use deepharness_lib::native::memory::MEMORY_KINDS;
use deepharness_lib::native::planner::{parse_plan, parse_step_json, MAX_STEPS};
use deepharness_lib::native::reflector::parse_review;
use deepharness_lib::native::tools::{access_mode_label, find_tool, TOOLS};
use deepharness_lib::paths::AgentDirs;
use deepharness_lib::permissions::AccessMode;

// ---- 规划器 ↔ 工具注册表的跨模块契约 ----

#[test]
fn plan_accepts_fenced_json_and_serializes_camel_case() {
    let raw = r#"模型输出如下：
```json
{
  "goal": "读取配置并汇报",
  "steps": [
    { "title": "读取配置", "tool": "read_text_file", "args": { "path": "a.txt" }, "expected": "拿到内容" },
    { "title": "总结", "tool": null, "args": null, "expected": "一句话结论" }
  ]
}
```
以上。"#;

    let plan = parse_plan(raw).expect("带围栏的计划应被接受");
    assert_eq!(plan.goal, "读取配置并汇报");
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].tool.as_deref(), Some("read_text_file"));
    assert!(plan.steps[1].tool.is_none(), "tool 为 null 的纯回答步骤");

    // IPC 往返依赖 camelCase：should_retry 之外，PlanStep/Plan 本身字段全为单词，
    // 这里校验序列化后的键名稳定（前端按 camelCase 直接消费）。
    let json = serde_json::to_value(&plan).expect("计划应可序列化");
    assert!(json.get("goal").is_some());
    assert!(json.get("steps").and_then(|v| v.as_array()).is_some());
    let first = &json["steps"][0];
    for key in ["title", "tool", "args", "expected"] {
        assert!(first.get(key).is_some(), "步骤缺少字段 {key}");
    }
}

#[test]
fn plan_rejects_tool_outside_registry() {
    let raw = r#"{"goal":"删库","steps":[{"title":"执行","tool":"rm_rf","args":{"path":"/"}}]}"#;
    let err = parse_plan(raw).expect_err("未注册的工具必须被拒绝");
    let msg = err.to_string();
    assert!(msg.contains("未知工具"), "错误信息应指出未知工具: {msg}");
    assert!(msg.contains("rm_rf"), "错误信息应带上工具名: {msg}");
}

#[test]
fn plan_rejects_more_steps_than_limit() {
    let steps: Vec<String> = (0..=MAX_STEPS)
        .map(|i| format!(r#"{{"title":"步骤{i}","tool":null,"args":null,"expected":""}}"#))
        .collect();
    let raw = format!(r#"{{"goal":"超长","steps":[{}]}}"#, steps.join(","));
    let err = parse_plan(&raw).expect_err("超过步骤上限必须被拒绝");
    assert!(err.to_string().contains("上限"), "{err}");
}

#[test]
fn tool_step_requires_args() {
    let raw = r#"{"title":"读取","tool":"read_text_file","expected":"内容"}"#;
    let err = parse_step_json(raw).expect_err("工具步骤缺少 args 必须被拒绝");
    assert!(err.to_string().contains("args"), "{err}");

    let ok = parse_step_json(r#"{"title":"读取","tool":"read_text_file","args":{"path":"a.txt"}}"#)
        .expect("补齐 args 后应通过");
    assert_eq!(ok.tool.as_deref(), Some("read_text_file"));
}

// ---- 工具注册表自身的一致性 ----

#[test]
fn tool_registry_is_self_consistent() {
    assert!(!TOOLS.is_empty(), "工具注册表不应为空");

    let mut seen = std::collections::HashSet::new();
    for spec in TOOLS {
        assert!(seen.insert(spec.name), "工具名重复: {}", spec.name);
        assert!(!spec.description.trim().is_empty(), "{} 缺少描述", spec.name);
        assert!(!spec.args_schema.trim().is_empty(), "{} 缺少参数说明", spec.name);
        let found = find_tool(spec.name).expect("注册表内的工具应能被查到");
        assert_eq!(found.name, spec.name);
    }
    assert!(find_tool("definitely_not_a_tool").is_none());
}

#[test]
fn access_mode_labels_round_trip_to_serde_names() {
    for (mode, expected) in [
        (AccessMode::Read, "read"),
        (AccessMode::ReadWrite, "read_write"),
    ] {
        assert_eq!(access_mode_label(mode), expected);
        // 前端与 Rust 之间的枚举以 serde 名对齐，二者必须一致
        let encoded = serde_json::to_string(&mode).unwrap();
        assert_eq!(encoded, format!("\"{expected}\""));
        let decoded: AccessMode = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, mode);
    }
}

// ---- 记忆库与反思输出 ----

#[test]
fn memory_kinds_are_unique() {
    let unique: std::collections::HashSet<_> = MEMORY_KINDS.iter().collect();
    assert_eq!(unique.len(), MEMORY_KINDS.len(), "记忆类型不应重复");
    for kind in ["fact", "preference", "event", "reflection"] {
        assert!(MEMORY_KINDS.contains(&kind), "缺少记忆类型 {kind}");
    }
}

#[test]
fn review_json_is_parsed_and_serialized_in_camel_case() {
    let review = parse_review(r#"{"success":false,"summary":"文件不存在","shouldRetry":true,"advice":"先确认路径"}"#)
        .expect("合法评审 JSON 应被接受");
    assert!(!review.success);
    assert!(review.should_retry);

    let json = serde_json::to_value(&review).unwrap();
    assert_eq!(json["shouldRetry"], serde_json::json!(true));
    assert_eq!(json["advice"], serde_json::json!("先确认路径"));
}

// ---- 配置持久化（真实文件，隔离在临时目录） ----

#[test]
fn agent_config_round_trips_through_isolated_directory() {
    let tmp = tempfile::tempdir().expect("创建临时目录");
    let dirs = AgentDirs::from_root(tmp.path().join("agents/deepharness"), "deepharness");

    let cfg = AgentModelConfig {
        base_url: "https://api.deepseek.com".to_string(),
        api_key: "sk-integration-test".to_string(),
        model: "deepseek-chat".to_string(),
    };
    cfg.save(&dirs).expect("保存配置");

    let path = AgentModelConfig::path(&dirs);
    assert!(path.starts_with(tmp.path()), "配置必须落在该 Agent 的隔离目录内");
    assert_eq!(AgentModelConfig::load(&dirs).as_ref(), Some(&cfg));

    // 损坏的配置不应 panic，而是返回 None 要求重新配置
    std::fs::write(&path, "{ 坏掉的 json").unwrap();
    assert!(AgentModelConfig::load(&dirs).is_none());
}

// ---- Agent 目录隔离 ----

#[test]
fn each_agent_gets_its_own_directory_tree() {
    let tmp = tempfile::tempdir().expect("创建临时目录");
    let mut roots = std::collections::HashSet::new();
    for id in deepharness_lib::paths::AGENT_IDS {
        let dirs = deepharness_lib::paths::ensure_agent_dirs(tmp.path(), id).expect("创建目录树");
        assert!(roots.insert(dirs.root.clone()), "{id} 的目录与其它 Agent 冲突");
        assert!(dirs.workspace.is_dir());
        assert!(dirs.config.is_dir());
        assert!(dirs.sessions.is_dir());
        assert!(dirs.logs.is_dir());
        assert!(dirs.plugins.is_dir());
    }
    assert_eq!(roots.len(), deepharness_lib::paths::AGENT_IDS.len());
}

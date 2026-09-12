//! 自研 Agent 的工具注册表。
//!
//! 安全模型：所有工具都在 Worker 进程内执行，但任何文件访问都必须先
//! 通过 `permissions::PermissionStore` 的白名单校验。白名单由主进程在
//! `configure` 请求中下发（主进程是授权的权威来源，前端授权弹窗产生的
//! 授权变更在下一次 `configure` / Worker 重启后生效）。
//!
//! 工具清单刻意保持最小闭环（读 / 写 / 列目录 / 建目录 / 删除），
//! 每个工具返回结构化 JSON 结果，失败时返回 `{"error": ..., "needsGrant": ...}`
//! 而不是中断整个任务循环。

use serde::Serialize;
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;
use crate::permissions::{AccessMode, PermissionStore};

/// 工具描述（进入规划器提示词，也用于前端展示）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub args_schema: &'static str,
}

/// 全部可用工具。
pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "read_text_file",
        description: "读取一个文本文件（UTF-8）的完整内容",
        args_schema: r#"{ "path": string }"#,
    },
    ToolSpec {
        name: "write_text_file",
        description: "把文本内容写入文件（覆盖写；父目录自动创建）",
        args_schema: r#"{ "path": string, "contents": string }"#,
    },
    ToolSpec {
        name: "list_directory",
        description: "列出目录内容（名称、类型、大小）",
        args_schema: r#"{ "path": string }"#,
    },
    ToolSpec {
        name: "create_directory",
        description: "创建目录（含多级父目录）",
        args_schema: r#"{ "path": string }"#,
    },
    ToolSpec {
        name: "delete_path",
        description: "删除文件或空目录；非空目录仅限 workspace 内删除",
        args_schema: r#"{ "path": string }"#,
    },
];

/// 按名称查找工具描述。
pub fn find_tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// 工具执行上下文。
pub struct ToolContext<'a> {
    pub perms: &'a PermissionStore,
    pub dirs: &'a AgentDirs,
}

/// 工具名 -> AgentDirs 的借用组合已由上下文给定，这里仅做统一校验入口。
fn authorized(
    ctx: &ToolContext<'_>,
    path: &str,
    write: bool,
) -> AppResult<std::path::PathBuf> {
    if path.trim().is_empty() {
        return Err(AppError::Other("path 不能为空".to_string()));
    }
    let target = std::path::Path::new(path);
    if !ctx.perms.check(&ctx.dirs.agent_id, ctx.dirs, target, write) {
        return Err(AppError::PathNotAuthorized(path.to_string()));
    }
    Ok(target.to_path_buf())
}

/// 执行一个工具调用。
///
/// 返回 JSON 结果；权限不足时错误信息包含"路径不在授权范围内"，
/// 编排层据此提示用户在前端授权。
pub fn execute_tool(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> AppResult<serde_json::Value> {
    let arg = |key: &str| -> AppResult<String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AppError::Other(format!("缺少参数 {key}")))
    };

    match name {
        "read_text_file" => {
            let path = arg("path")?;
            let target = authorized(ctx, &path, false)?;
            if !target.is_file() {
                return Err(AppError::PathNotFound(path));
            }
            let content = std::fs::read_to_string(&target)?;
            Ok(json!({ "path": path, "content": content }))
        }
        "write_text_file" => {
            let path = arg("path")?;
            let contents = arg("contents")?;
            let target = authorized(ctx, &path, true)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let bytes = contents.as_bytes().len() as u64;
            std::fs::write(&target, contents.as_bytes())?;
            tracing::info!(path = %path, bytes, "tool: write_text_file");
            Ok(json!({ "path": path, "written": bytes }))
        }
        "list_directory" => {
            let path = arg("path")?;
            let target = authorized(ctx, &path, false)?;
            if !target.is_dir() {
                return Err(AppError::PathNotFound(path));
            }
            let mut entries: Vec<serde_json::Value> = Vec::new();
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
        "create_directory" => {
            let path = arg("path")?;
            let target = authorized(ctx, &path, true)?;
            std::fs::create_dir_all(&target)?;
            tracing::info!(path = %path, "tool: create_directory");
            Ok(json!({ "path": path, "created": true }))
        }
        "delete_path" => {
            let path = arg("path")?;
            let target = authorized(ctx, &path, true)?;
            let canonical = crate::permissions::canonicalize_lenient(&target)
                .ok_or_else(|| AppError::PathNotFound(path.clone()))?;
            if canonical.is_file() {
                std::fs::remove_file(&canonical)?;
            } else if canonical.is_dir() {
                let empty = std::fs::read_dir(&canonical)?.next().is_none();
                if empty {
                    std::fs::remove_dir(&canonical)?;
                } else {
                    // 非空目录：仅 workspace 内允许递归删除（与 fs_ops 一致）
                    let in_workspace = canonical.strip_prefix(&ctx.dirs.workspace).is_ok();
                    if !in_workspace {
                        return Err(AppError::Other(
                            "workspace 之外的非空目录不允许递归删除".to_string(),
                        ));
                    }
                    std::fs::remove_dir_all(&canonical)?;
                }
            } else {
                return Err(AppError::PathNotFound(path));
            }
            tracing::info!(path = %path, "tool: delete_path");
            Ok(json!({ "path": path, "deleted": true }))
        }
        other => Err(AppError::Other(format!("未知工具: {other}"))),
    }
}

/// 把授权模式渲染为字符串（配置下发用）。
pub fn access_mode_label(mode: AccessMode) -> &'static str {
    match mode {
        AccessMode::Read => "read",
        AccessMode::ReadWrite => "read_write",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::PermissionStore;

    fn setup(tag: &str) -> (tempfile::TempDir, AgentDirs, PermissionStore) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(tag);
        let dirs =
            AgentDirs::from_root(root.clone(), "deepharness");
        std::fs::create_dir_all(&dirs.workspace).unwrap();
        let perms = PermissionStore::new(root.join("permissions.json"));
        (tmp, dirs, perms)
    }

    #[test]
    fn read_and_write_within_workspace() {
        let (_t, dirs, perms) = setup("rw");
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let file = dirs.workspace.join("note.txt");

        let out = execute_tool(
            &ctx,
            "write_text_file",
            &json!({ "path": file.display().to_string(), "contents": "你好" }),
        )
        .unwrap();
        assert_eq!(out["written"], 6); // UTF-8 中文 3 字节/字

        let out = execute_tool(
            &ctx,
            "read_text_file",
            &json!({ "path": file.display().to_string() }),
        )
        .unwrap();
        assert_eq!(out["content"], "你好");
    }

    #[test]
    fn workspace_outside_requires_grant() {
        let (tmp, dirs, perms) = setup("outside");
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let outside = tmp.path().join("secret.txt");
        std::fs::write(&outside, "x").unwrap();

        let err = execute_tool(
            &ctx,
            "read_text_file",
            &json!({ "path": outside.display().to_string() }),
        )
        .unwrap_err();
        assert!(matches!(err, AppError::PathNotAuthorized(_)));

        // 授权后可读
        perms
            .grant("deepharness", &outside, AccessMode::Read, None)
            .unwrap();
        let out = execute_tool(
            &ctx,
            "read_text_file",
            &json!({ "path": outside.display().to_string() }),
        )
        .unwrap();
        assert_eq!(out["content"], "x");
    }

    #[test]
    fn list_directory_sorts_dirs_first() {
        let (_t, dirs, perms) = setup("list");
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        std::fs::create_dir_all(dirs.workspace.join("zdir")).unwrap();
        std::fs::write(dirs.workspace.join("a.txt"), "1").unwrap();
        let out = execute_tool(
            &ctx,
            "list_directory",
            &json!({ "path": dirs.workspace.display().to_string() }),
        )
        .unwrap();
        let entries = out["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["name"], "zdir");
        assert!(entries[0]["isDir"].as_bool().unwrap());
    }

    #[test]
    fn delete_nonempty_dir_outside_workspace_is_refused() {
        let (tmp, dirs, perms) = setup("del");
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        let outside_dir = tmp.path().join("granted_dir");
        std::fs::create_dir_all(outside_dir.join("sub")).unwrap();
        perms
            .grant("deepharness", &outside_dir, AccessMode::ReadWrite, None)
            .unwrap();
        let err = execute_tool(
            &ctx,
            "delete_path",
            &json!({ "path": outside_dir.display().to_string() }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("不允许递归删除"));

        // workspace 内非空目录可递归删
        let inside = dirs.workspace.join("proj");
        std::fs::create_dir_all(inside.join("deep")).unwrap();
        execute_tool(
            &ctx,
            "delete_path",
            &json!({ "path": inside.display().to_string() }),
        )
        .unwrap();
        assert!(!inside.exists());
    }

    #[test]
    fn missing_args_and_unknown_tools_error_cleanly() {
        let (_t, dirs, perms) = setup("args");
        let ctx = ToolContext { perms: &perms, dirs: &dirs };
        assert!(execute_tool(&ctx, "read_text_file", &json!({})).is_err());
        assert!(execute_tool(&ctx, "warp", &json!({})).is_err());
        assert!(find_tool("read_text_file").is_some());
        assert!(find_tool("warp").is_none());
    }
}

//! 文件系统操作的 Tauri 命令层（安全封装）。
//!
//! 所有命令的固定模式：
//! 1. 校验 agent 标识；
//! 2. 取该 Agent 的目录布局；
//! 3. 用 `PermissionStore` 校验目标路径（读 / 写）；
//! 4. 执行操作并记录 tracing 日志。
//!
//! 前端永远接触不到任意路径的裸 IO —— 一切未授权访问在命令层被拒绝。

use std::path::Path;

use base64::Engine as _;
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::paths::{ensure_agent_dirs, AgentDirs};
use crate::permissions::{AccessMode, PermissionStore};

/// 目录列表项。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// 授权判定结果（供前端决定是否弹出授权对话框）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessDecision {
    pub granted: bool,
    pub path: String,
}

/// 单条授权的对外表示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantView {
    pub path: String,
    pub mode: AccessMode,
    pub granted_at: String,
    pub label: Option<String>,
}

/// 统一入口：校验 agent + 解析目录布局。
fn resolve_dirs(base: &BaseDir, agent_id: &str) -> AppResult<AgentDirs> {
    crate::paths::validate_agent_id(agent_id)?;
    ensure_agent_dirs(&base.0, agent_id)
}

/// 持有应用数据根目录的轻量容器（由 Tauri manage 注入）。
pub struct BaseDir(pub std::path::PathBuf);

/// 预检：路径是否已被授权（不弹窗、不落盘）。
#[tauri::command]
pub fn check_access(
    agent: String,
    path: String,
    write: bool,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<AccessDecision> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    let granted = perms.check(&agent, &dirs, target, write);
    Ok(AccessDecision {
        granted,
        path: path.clone(),
    })
}

/// 用户在授权弹窗中确认后调用：为 Agent 授予路径访问权。
#[tauri::command]
pub fn grant_access(
    agent: String,
    path: String,
    mode: AccessMode,
    label: Option<String>,
    perms: tauri::State<'_, PermissionStore>,
) -> AppResult<GrantView> {
    let grant = perms.grant(&agent, Path::new(&path), mode, label.as_deref())?;
    Ok(GrantView {
        path: grant.path.display().to_string(),
        mode: grant.mode,
        granted_at: grant.granted_at.to_rfc3339(),
        label: grant.label,
    })
}

/// 撤销一条授权。
#[tauri::command]
pub fn revoke_access(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
) -> AppResult<()> {
    perms.revoke(&agent, Path::new(&path))
}

/// 列出某 Agent 当前全部授权。
#[tauri::command]
pub fn list_grants(
    agent: String,
    perms: tauri::State<'_, PermissionStore>,
) -> AppResult<Vec<GrantView>> {
    let grants = perms.list(&agent)?;
    Ok(grants
        .into_iter()
        .map(|g| GrantView {
            path: g.path.display().to_string(),
            mode: g.mode,
            granted_at: g.granted_at.to_rfc3339(),
            label: g.label,
        })
        .collect())
}

/// 读取文本文件（UTF-8）。
#[tauri::command]
pub fn read_text_file(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<String> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, false) {
        return Err(AppError::PathNotAuthorized(path));
    }
    if !target.is_file() {
        return Err(AppError::PathNotFound(path));
    }
    let content = std::fs::read_to_string(target)?;
    tracing::debug!(agent = %agent, path = %path, bytes = content.len(), "read_text_file");
    Ok(content)
}

/// 读取二进制文件，返回 Base64 编码（用于图片等非文本资源）。
#[tauri::command]
pub fn read_file_base64(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<String> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, false) {
        return Err(AppError::PathNotAuthorized(path));
    }
    if !target.is_file() {
        return Err(AppError::PathNotFound(path));
    }
    let bytes = std::fs::read(target)?;
    tracing::debug!(agent = %agent, path = %path, bytes = bytes.len(), "read_file_base64");
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// 写入文本文件（UTF-8，覆盖写）。父目录不存在时自动创建。
///
/// 权限校验说明：`check` 对尚不存在的路径采用“最深存在祖先 + 余段”
/// 的规范化，白名单前缀匹配天然覆盖新文件的各级父目录——若授权的是
/// 只读，写请求在此即被拒绝；若授权目录本身可写，其下新建子目录 /
/// 文件自然合法，因此无需再逐级校验祖先。
#[tauri::command]
pub fn write_text_file(
    agent: String,
    path: String,
    contents: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<u64> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, true) {
        return Err(AppError::PathNotAuthorized(path));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let written = contents.as_bytes().len() as u64;
    std::fs::write(target, contents.as_bytes())?;
    tracing::info!(agent = %agent, path = %path, bytes = written, "write_text_file");
    Ok(written)
}

/// 列出目录内容。
#[tauri::command]
pub fn list_directory(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<Vec<DirEntryInfo>> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, false) {
        return Err(AppError::PathNotAuthorized(path));
    }
    if !target.is_dir() {
        return Err(AppError::PathNotFound(path));
    }
    let mut out: Vec<DirEntryInfo> = Vec::new();
    for entry in std::fs::read_dir(target)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        out.push(DirEntryInfo { name, is_dir, size });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    tracing::debug!(agent = %agent, path = %path, entries = out.len(), "list_directory");
    Ok(out)
}

/// 创建目录（含多级父目录）。
#[tauri::command]
pub fn create_directory(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<String> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, true) {
        return Err(AppError::PathNotAuthorized(path));
    }
    std::fs::create_dir_all(target)?;
    tracing::info!(agent = %agent, path = %path, "create_directory");
    Ok(path)
}

/// 删除文件或空目录。
///
/// 出于安全考虑：非空目录的递归删除只允许发生在 Agent 自己的
/// workspace 内部；workspace 之外（即用户显式授权的区域）只允许
/// 删除单个文件或空目录。
#[tauri::command]
pub fn delete_path(
    agent: String,
    path: String,
    perms: tauri::State<'_, PermissionStore>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<()> {
    let dirs = resolve_dirs(&base, &agent)?;
    let target = Path::new(&path);
    if !perms.check(&agent, &dirs, target, true) {
        return Err(AppError::PathNotAuthorized(path));
    }
    let canonical = crate::permissions::canonicalize_lenient(target)
        .ok_or_else(|| AppError::PathNotFound(path.clone()))?;

    if canonical.is_file() {
        std::fs::remove_file(&canonical)?;
    } else if canonical.is_dir() {
        let empty = std::fs::read_dir(&canonical)?
            .next()
            .is_none();
        if empty {
            std::fs::remove_dir(&canonical)?;
        } else {
            // 非空目录：仅 workspace 内允许递归删除。路径包含判定统一走
            // permissions::is_within（两侧先规范化，避免 \\?\ 前缀、
            // 短名与大小写差异导致的误判）。
            if !crate::permissions::is_within(&dirs.workspace, &canonical) {
                return Err(AppError::Other(
                    "拒绝删除：目录非空，且不在该 Agent 的工作区内（只允许删除文件或空目录）"
                        .to_string(),
                ));
            }
            std::fs::remove_dir_all(&canonical)?;
        }
    } else {
        return Err(AppError::PathNotFound(path));
    }
    tracing::info!(agent = %agent, path = %path, "delete_path");
    Ok(())
}

/// 返回某 Agent 的目录布局（供前端展示与默认工作区定位）。
#[tauri::command]
pub fn agent_dirs_info(
    agent: String,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<AgentDirs> {
    resolve_dirs(&base, &agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_dirs_rejects_bad_agent() {
        let base = BaseDir(std::env::temp_dir().join("dh_test_resolve"));
        assert!(resolve_dirs(&base, "nope").is_err());
    }

    #[test]
    fn base_dir_state_is_simple_container() {
        let base = BaseDir(std::env::temp_dir().join("dh_test_container"));
        assert!(base.0.starts_with(std::env::temp_dir()));
    }
}

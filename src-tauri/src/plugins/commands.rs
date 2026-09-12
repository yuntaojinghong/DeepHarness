//! 插件管理的 Tauri 命令层。
//!
//! ## 为什么安装要走 base64
//!
//! 前端拿不到任意路径的文件读取能力（这是本项目的安全前提）。所以安装流是：
//! 前端 `<input type="file">` 读成字节 → base64 → 本命令解码。
//! 后端**不接受路径**，也就不存在"前端诱导后端读任意文件"的面。
//!
//! 卸载 / 启停只接受插件 id，目录名一律由后端拼接（`PluginStore` 内部还会
//! 再校验一次 id 合法性），拒绝任何路径字符。
//!
//! ## 生效时机
//!
//! 每次变更后，如果 Worker 正在运行就重新 `configure` 一次 —— 插件在
//! configure 时被读进运行时，因此**装完即生效，不需要重启应用**。
//! Worker 未运行（或未配置模型）时只落盘，下次启动自然带上。

use base64::Engine as _;

use crate::agents::agent_config::AgentModelConfig;
use crate::agents::registry::NativeAgentHandle;
use crate::error::{AppError, AppResult};
use crate::fs_ops::BaseDir;
use crate::paths::ensure_agent_dirs;
use crate::plugins::store::MAX_ARCHIVE_BYTES;
use crate::plugins::{PluginRecord, PluginStore};

/// 校验 agent 并解析目录布局（与 fs_ops 同款入口）。
fn resolve_store(base: &BaseDir, agent_id: &str) -> AppResult<PluginStore> {
    let dirs = ensure_agent_dirs(&base.0, agent_id)?;
    Ok(PluginStore::new(&dirs))
}

/// 把前端读到的 base64 解码为 zip 字节。
///
/// 宽容处理两种常见形态：`data:…;base64,xxx` 前缀、夹带换行/空格的 base64；
/// 但**先按长度挡一道**再解码，避免超大输入把内存吃光。
fn decode_archive(raw: &str) -> AppResult<Vec<u8>> {
    let trimmed = raw.trim();
    // 去掉 data URL 前缀（readAsDataURL 会带上）
    let payload = match trimmed.find("base64,") {
        Some(idx) => &trimmed[idx + "base64,".len()..],
        None => trimmed,
    };
    // base64 长度 ≈ 原始字节 * 4/3，先按输入长度估算，超出上限直接拒绝
    let max_b64 = MAX_ARCHIVE_BYTES / 3 * 4 + 8;
    if payload.len() > max_b64 {
        return Err(AppError::Other(format!(
            "插件包过大（上限 {} MB）",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        )));
    }
    let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(AppError::Other("插件包内容为空".to_string()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| AppError::Other(format!("插件包不是有效的 base64 数据: {e}")))?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AppError::Other(format!(
            "插件包过大（上限 {} MB）",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        )));
    }
    // zip 魔数：普通归档为 PK\x03\x04；空归档为 PK\x05\x06；分卷为 PK\x07\x08。
    // 提前挡掉"随便选了个文件"，比让解包层报"不是 zip"更友好。
    let is_zip = bytes.len() >= 4
        && &bytes[0..2] == b"PK"
        && matches!(bytes[2], 0x03 | 0x05 | 0x07);
    if !is_zip {
        return Err(AppError::Other(
            ".dph-plugin 必须是 zip 压缩包（扩展名通常为 .dph-plugin 或 .zip）".to_string(),
        ));
    }
    Ok(bytes)
}

/// 让运行中的 Worker 立刻看到插件变更（未运行 / 未配置时静默跳过）。
fn hot_reload(handle: &NativeAgentHandle, reason: &str) {
    let rt = &handle.0;
    if !rt.is_alive() {
        tracing::info!(reason, "Worker 未运行，插件变更已落盘，下次启动生效");
        return;
    }
    let Some(cfg) = AgentModelConfig::load(rt.dirs()) else {
        tracing::info!(reason, "Worker 尚未配置模型，插件变更待下次 configure 生效");
        return;
    };
    match rt.reconfigure(&cfg) {
        Ok(info) => tracing::info!(
            reason,
            plugin_tools = info.get("pluginTools").and_then(|v| v.as_u64()).unwrap_or(0),
            "插件变更已热加载"
        ),
        Err(e) => tracing::warn!(reason, error = %e, "插件变更热加载失败，重启该 Agent 后生效"),
    }
}

/// 列出某 Agent 已安装的插件（含损坏条目 —— 隐藏它们只会让人更困惑）。
#[tauri::command]
pub fn plugin_list(
    agent: String,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<Vec<PluginRecord>> {
    let store = resolve_store(&base, &agent)?;
    Ok(store.list())
}

/// 插件目录（供界面展示 / 用户手动放包）。
#[tauri::command]
pub fn plugin_dir(agent: String, base: tauri::State<'_, BaseDir>) -> AppResult<String> {
    let store = resolve_store(&base, &agent)?;
    Ok(store.root().display().to_string())
}

/// 安装（或升级）一个 `.dph-plugin`：内容为 base64 编码的 zip 字节。
#[tauri::command]
pub fn plugin_install(
    agent: String,
    archive_base64: String,
    handle: tauri::State<'_, NativeAgentHandle>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<PluginRecord> {
    let store = resolve_store(&base, &agent)?;
    let bytes = decode_archive(&archive_base64)?;
    let record = store.install(&bytes)?;
    tracing::info!(agent = %agent, plugin = %record.id, version = %record.version, "插件已安装");
    hot_reload(&handle, "plugin_install");
    Ok(record)
}

/// 启用 / 停用某个插件。
#[tauri::command]
pub fn plugin_set_enabled(
    agent: String,
    id: String,
    enabled: bool,
    handle: tauri::State<'_, NativeAgentHandle>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<PluginRecord> {
    let store = resolve_store(&base, &agent)?;
    let record = store.set_enabled(&id, enabled)?;
    hot_reload(&handle, "plugin_set_enabled");
    Ok(record)
}

/// 卸载某个插件（删除其目录与状态项）。
#[tauri::command]
pub fn plugin_uninstall(
    agent: String,
    id: String,
    handle: tauri::State<'_, NativeAgentHandle>,
    base: tauri::State<'_, BaseDir>,
) -> AppResult<()> {
    let store = resolve_store(&base, &agent)?;
    store.uninstall(&id)?;
    tracing::info!(agent = %agent, plugin = %id, "插件已卸载");
    hot_reload(&handle, "plugin_uninstall");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn decode_archive_accepts_zip_magic() {
        let zip = b"PK\x03\x04rest-of-archive";
        assert_eq!(decode_archive(&b64(zip)).unwrap(), zip);
    }

    #[test]
    fn decode_archive_tolerates_data_url_and_whitespace() {
        let zip = b"PK\x03\x04abc";
        let raw = format!("data:application/zip;base64,{}", b64(zip));
        assert_eq!(decode_archive(&raw).unwrap(), zip);

        // 夹带换行（有些前端会按 76 列折行）
        let folded = b64(zip)
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(decode_archive(&folded).unwrap(), zip);
    }

    #[test]
    fn decode_archive_rejects_junk_and_non_zip() {
        assert!(decode_archive("").is_err());
        assert!(decode_archive("   ").is_err());
        assert!(decode_archive("!!!不是 base64!!!").is_err());
        // 合法 base64 但不是 zip（例如一段纯文本）
        let err = decode_archive(&b64(b"hello world, not a zip")).unwrap_err();
        assert!(err.to_string().contains("zip"), "{err}");
    }

    #[test]
    fn decode_archive_rejects_oversized_input_before_decoding() {
        let huge = "A".repeat(MAX_ARCHIVE_BYTES / 3 * 4 + 64);
        let err = decode_archive(&huge).unwrap_err();
        assert!(err.to_string().contains("过大"), "{err}");
    }
}

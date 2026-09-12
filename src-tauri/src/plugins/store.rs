//! 插件仓库：`<data>/agents/<agent>/plugins/` 下的安装、启停与卸载。
//!
//! 目录布局：
//!
//! ```text
//! plugins/
//!   plugins.json          # 启停状态（唯一的可变状态文件）
//!   <plugin-id>/          # 一个插件一个目录，解包自 .dph-plugin
//!     manifest.json
//!     index.js
//! ```
//!
//! **每 Agent 独立**：本模块的一切路径都从 `AgentDirs::plugins` 推导，
//! 不存在跨 Agent 共享的插件目录或状态文件。一个 Agent 禁用 / 卸载插件
//! 不会影响另外两个。
//!
//! ## 为什么要自己解 zip 而不是用现成 API
//!
//! 用的是 `zip` crate（`deflate` 特性，纯 Rust 走已在依赖树里的 flate2），
//! 但**路径安全由本模块自己把关**：zip 条目名可以写成 `../../x` 或
//! 绝对路径（即 zip-slip），必须逐条过滤后才能落盘。

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;

use super::{PluginManifest, validate_plugin_id};

/// 状态文件名。
const STATE_FILE: &str = "plugins.json";

/// 解包前的静态上限，挡住 zip 炸弹。
const MAX_ENTRIES: usize = 512;
const MAX_SINGLE_FILE: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_UNPACKED: u64 = 32 * 1024 * 1024;

/// 安装前的压缩包体积上限（前端读文件时先挡一道）。
pub const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;

/// 面向 UI 的插件条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    /// 目录名（= manifest.id）；manifest 损坏时取自目录名。
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: String,
    pub enabled: bool,
    /// manifest 校验通过时为 true。
    pub valid: bool,
    /// `valid == false` 时的可读原因。
    pub error: Option<String>,
    /// 声明的工具名列表（manifest 无效时为空）。
    pub tools: Vec<String>,
    /// 插件目录的绝对路径。
    pub dir: String,
}

/// 磁盘上已安装且 manifest 有效的插件。
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub enabled: bool,
}

/// 状态文件结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginState {
    /// 插件 id -> 是否启用。**缺失的 id 视为启用**（新装的插件默认可用）。
    #[serde(default)]
    enabled: std::collections::BTreeMap<String, bool>,
}

/// 某个 Agent 的插件仓库。
#[derive(Debug, Clone)]
pub struct PluginStore {
    root: PathBuf,
}

impl PluginStore {
    /// 绑定到某个 Agent 的插件目录（不创建目录）。
    pub fn new(dirs: &AgentDirs) -> Self {
        Self {
            root: dirs.plugins.clone(),
        }
    }

    /// 仅用于测试 / 诊断：插件根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 列出全部插件（含 manifest 损坏的条目，便于 UI 暴露问题而不是静默隐藏）。
    pub fn list(&self) -> Vec<PluginRecord> {
        let state = self.load_state();
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            // 跳过内部目录
            if dir_name.starts_with('.') {
                continue;
            }
            let enabled = state.enabled.get(&dir_name).copied().unwrap_or(true);
            out.push(self.record_for(&dir_name, &path, enabled));
        }
        // 稳定排序：按 id 升序，保证同一份磁盘状态每次列表顺序一致
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// 已安装且 manifest 有效的插件（供 Worker 构建运行时）。
    pub fn enabled(&self) -> AppResult<Vec<InstalledPlugin>> {
        let state = self.load_state();
        let mut out = Vec::new();
        for record in self.list() {
            if !record.valid || !record.enabled {
                continue;
            }
            let dir = PathBuf::from(&record.dir);
            match read_manifest(&dir) {
                Ok(manifest) => out.push(InstalledPlugin {
                    manifest,
                    dir,
                    enabled: state.enabled.get(&record.id).copied().unwrap_or(true),
                }),
                Err(e) => {
                    // list() 已判定有效，这里再失败说明磁盘在两次读取之间被改动；
                    // 记一条警告即可，不阻断其它插件。
                    tracing::warn!(plugin = %record.id, error = %e, "读取插件 manifest 失败，已跳过");
                }
            }
        }
        out.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(out)
    }

    /// 从 `.dph-plugin`（zip）字节流安装或升级一个插件。
    ///
    /// 流程刻意分两遍：**先只读 manifest 并完成全部校验，再落盘解包**。
    /// 这样非法包不会在磁盘上留下任何半成品。
    pub fn install(&self, archive: &[u8]) -> AppResult<PluginRecord> {
        if archive.is_empty() {
            return Err(AppError::Other("插件包为空".to_string()));
        }
        if archive.len() > MAX_ARCHIVE_BYTES {
            return Err(AppError::Other(format!(
                "插件包过大（{} 字节，上限 {MAX_ARCHIVE_BYTES} 字节）",
                archive.len()
            )));
        }

        let cursor = std::io::Cursor::new(archive);
        let mut zip = zip::ZipArchive::new(cursor)
            .map_err(|e| AppError::Other(format!("不是有效的 zip 包: {e}")))?;

        if zip.len() > MAX_ENTRIES {
            return Err(AppError::Other(format!(
                "插件包文件数过多（{} 个，上限 {MAX_ENTRIES} 个）",
                zip.len()
            )));
        }

        // ── 第一遍：解出 manifest 并校验 ────────────────────────────────
        let manifest_raw = read_zip_text(&mut zip, "manifest.json")?
            .ok_or_else(|| AppError::Other("插件包根目录缺少 manifest.json".to_string()))?;
        let manifest: PluginManifest = serde_json::from_str(&manifest_raw)
            .map_err(|e| AppError::Other(format!("manifest.json 解析失败: {e}")))?;
        manifest.validate()?;

        // 入口文件必须在包内声明，否则装完必然不可用
        if !zip_contains(&mut zip, &manifest.entry) {
            return Err(AppError::Other(format!(
                "插件包内找不到入口文件 `{}`",
                manifest.entry
            )));
        }

        // ── 第二遍：解包到 plugins/<id> ────────────────────────────────
        std::fs::create_dir_all(&self.root)?;
        let target = self.root.join(&manifest.id);

        // 升级场景：先解到临时目录，成功后再替换，避免中途失败把旧版本砸掉
        let staging = self.root.join(format!(".staging-{}", manifest.id));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;

        let extracted = extract_zip(&mut zip, &staging);
        if let Err(e) = extracted {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        if !staging.join(&manifest.entry).is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(AppError::Other(format!(
                "解包后仍找不到入口文件 `{}`",
                manifest.entry
            )));
        }

        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&staging, &target)?;

        // 新装的插件默认启用；显式写一条状态，便于用户看到确定值
        let mut state = self.load_state();
        state.enabled.insert(manifest.id.clone(), true);
        self.save_state(&state)?;

        tracing::info!(
            plugin = %manifest.id,
            version = %manifest.version,
            tools = manifest.tools.len(),
            "插件已安装"
        );
        Ok(self.record_for(&manifest.id, &target, true))
    }

    /// 启用 / 停用一个插件。
    pub fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<PluginRecord> {
        validate_plugin_id(id)?;
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(AppError::Other(format!("插件 `{id}` 不存在")));
        }
        let mut state = self.load_state();
        state.enabled.insert(id.to_string(), enabled);
        self.save_state(&state)?;
        tracing::info!(plugin = %id, enabled, "插件启停状态已更新");
        Ok(self.record_for(id, &dir, enabled))
    }

    /// 卸载（删除插件目录与状态）。
    pub fn uninstall(&self, id: &str) -> AppResult<()> {
        validate_plugin_id(id)?;
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(AppError::Other(format!("插件 `{id}` 不存在")));
        }
        std::fs::remove_dir_all(&dir)?;
        let mut state = self.load_state();
        state.enabled.remove(id);
        self.save_state(&state)?;
        tracing::info!(plugin = %id, "插件已卸载");
        Ok(())
    }

    // ── 内部 ──────────────────────────────────────────────────────────

    fn record_for(&self, id: &str, dir: &Path, enabled: bool) -> PluginRecord {
        match read_manifest(dir) {
            Ok(manifest) => PluginRecord {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                description: manifest.description.clone(),
                author: manifest.author.clone(),
                homepage: manifest.homepage.clone(),
                enabled,
                valid: true,
                error: None,
                tools: manifest.tools.iter().map(|t| t.name.clone()).collect(),
                dir: dir.display().to_string(),
            },
            Err(e) => PluginRecord {
                id: id.to_string(),
                name: id.to_string(),
                version: String::new(),
                description: String::new(),
                author: String::new(),
                homepage: String::new(),
                enabled: false,
                valid: false,
                error: Some(e.to_string()),
                tools: Vec::new(),
                dir: dir.display().to_string(),
            },
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }

    /// 读状态；文件缺失或损坏一律按「全部启用」处理（不 panic、不清盘）。
    fn load_state(&self) -> PluginState {
        let path = self.state_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return PluginState::default();
        };
        match serde_json::from_str::<PluginState>(&raw) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "插件状态文件损坏，按默认值处理");
                PluginState::default()
            }
        }
    }

    /// 原子写状态：先写临时文件再 rename，避免断电 / 崩溃留下半截 JSON。
    fn save_state(&self, state: &PluginState) -> AppResult<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.state_path();
        let tmp = self.root.join(format!("{STATE_FILE}.tmp"));
        let body = serde_json::to_string_pretty(state)?;
        std::fs::write(&tmp, body.as_bytes())?;
        // Windows 上 rename 目标已存在会失败，先删
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

/// 从目录读取并校验 manifest。
fn read_manifest(dir: &Path) -> AppResult<PluginManifest> {
    let raw = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| AppError::Other(format!("读取 manifest.json 失败: {e}")))?;
    let manifest: PluginManifest = serde_json::from_str(&raw)
        .map_err(|e| AppError::Other(format!("manifest.json 解析失败: {e}")))?;
    manifest.validate()?;
    // 目录名必须与 id 一致，否则状态文件与目录会各说各话
    let dir_name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if dir_name != manifest.id {
        return Err(AppError::Other(format!(
            "插件目录名 `{dir_name}` 与 manifest.id `{}` 不一致",
            manifest.id
        )));
    }
    Ok(manifest)
}

/// 判断一个 zip 内的路径是否可以安全地作为相对路径使用。
///
/// 逐个组件过滤：只接受普通组件与 `.`，任何根 / 前缀 / `..` 都判非法。
/// 这是防 zip-slip 的唯一判据，**不要**用字符串 `contains("..")` 代替——
/// 那样会把合法文件名 `a..b.js` 误杀，又漏掉 `\` 分隔的平台差异。
pub fn is_safe_relative(name: &str) -> bool {
    safe_relative(name).is_some()
}

/// 把 zip 条目名归一化为安全的相对路径。
fn safe_relative(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in Path::new(name).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            // RootDir / Prefix（盘符、UNC）/ ParentDir 一律拒绝
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 归一化 zip 条目名：去掉 `./` 前缀，统一分隔符便于与 manifest 里的写法比对。
fn normalized_name(name: &str) -> Option<String> {
    let rel = safe_relative(name)?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        if let Component::Normal(p) = comp {
            parts.push(p.to_string_lossy().to_string());
        }
    }
    Some(parts.join("/"))
}

/// 按归一化名字在 zip 中查找文本文件。
fn read_zip_text(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    wanted: &str,
) -> AppResult<Option<String>> {
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Other(format!("读取插件包条目失败: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        if normalized_name(entry.name()).as_deref() != Some(wanted) {
            continue;
        }
        if entry.size() > MAX_SINGLE_FILE {
            return Err(AppError::Other(format!(
                "`{wanted}` 过大（{} 字节）",
                entry.size()
            )));
        }
        let mut s = String::new();
        entry
            .read_to_string(&mut s)
            .map_err(|e| AppError::Other(format!("读取 `{wanted}` 失败: {e}")))?;
        return Ok(Some(s));
    }
    Ok(None)
}

/// zip 内是否存在某个归一化名字的文件。
fn zip_contains(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, wanted: &str) -> bool {
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        if normalized_name(entry.name()).as_deref() == Some(wanted) {
            return true;
        }
    }
    false
}

/// 解包全部条目到 `dest`，逐条做路径与体积校验。
fn extract_zip(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    dest: &Path,
) -> AppResult<()> {
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Other(format!("读取插件包条目失败: {e}")))?;
        let raw_name = entry.name().to_string();
        let Some(rel) = safe_relative(&raw_name) else {
            return Err(AppError::Other(format!(
                "插件包含非法路径条目 `{raw_name}`（绝对路径或向上跳目录）"
            )));
        };

        let out_path = dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        let declared = entry.size();
        if declared > MAX_SINGLE_FILE {
            return Err(AppError::Other(format!(
                "条目 `{raw_name}` 过大（{declared} 字节，上限 {MAX_SINGLE_FILE} 字节）"
            )));
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut buf = Vec::with_capacity(declared.min(1024 * 1024) as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| AppError::Other(format!("解压 `{raw_name}` 失败: {e}")))?;

        // 以实际读出的字节数为准累加：entry.size() 来自中央目录，
        // 恶意包可以谎报，必须用真实体积做上限判断。
        total = total.saturating_add(buf.len() as u64);
        if total > MAX_TOTAL_UNPACKED {
            return Err(AppError::Other(format!(
                "插件包解压后体积过大（已超过 {MAX_TOTAL_UNPACKED} 字节）"
            )));
        }

        std::fs::write(&out_path, &buf)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造一个最小可用的 `.dph-plugin`（zip 字节流）用于测试。
    fn make_plugin_zip(id: &str, extra: &[(&str, &str)]) -> Vec<u8> {
        let manifest = format!(
            r#"{{"id":"{id}","name":"测试插件","version":"1.0.0","description":"d","tools":[{{"name":"ab_tool","description":"干活"}}]}}"#
        );
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("manifest.json", opts).unwrap();
            w.write_all(manifest.as_bytes()).unwrap();
            w.start_file("index.js", opts).unwrap();
            w.write_all(b"module.exports={tools:{ab_tool:async()=>({ok:true})}};").unwrap();
            for (name, body) in extra {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    fn store(tag: &str) -> (tempfile::TempDir, PluginStore) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(tag);
        let dirs = AgentDirs::from_root(root.clone(), "deepharness");
        std::fs::create_dir_all(&dirs.plugins).unwrap();
        let store = PluginStore::new(&dirs);
        (tmp, store)
    }

    #[test]
    fn safe_relative_rejects_traversal_and_absolute() {
        assert!(is_safe_relative("index.js"));
        assert!(is_safe_relative("dist/main.js"));
        assert!(is_safe_relative("./a/b.js"));
        // 合法文件名里出现连续点，不应被误杀
        assert!(is_safe_relative("a..b.js"));

        assert!(!is_safe_relative("../escape.js"));
        assert!(!is_safe_relative("a/../../b.js"));
        assert!(!is_safe_relative("/abs.js"));
        assert!(!is_safe_relative("C:/x.js"));
        assert!(!is_safe_relative(""));
        assert!(!is_safe_relative("./"));
    }

    #[test]
    fn normalized_name_strips_dot_slash() {
        assert_eq!(normalized_name("./manifest.json").as_deref(), Some("manifest.json"));
        assert_eq!(normalized_name("manifest.json").as_deref(), Some("manifest.json"));
        assert_eq!(normalized_name("a\\b.js").as_deref(), Some("a/b.js"));
        assert_eq!(normalized_name("../x"), None);
    }

    #[test]
    fn install_then_list_and_enabled() {
        let (_t, store) = store("install");
        let zip = make_plugin_zip("com.test.one", &[]);
        let rec = store.install(&zip).unwrap();
        assert_eq!(rec.id, "com.test.one");
        assert!(rec.valid);
        assert!(rec.enabled, "新装插件应默认启用");
        assert_eq!(rec.tools, vec!["ab_tool".to_string()]);

        // 入口文件确实落盘
        assert!(store.root().join("com.test.one").join("index.js").is_file());
        assert!(store.root().join("com.test.one").join("manifest.json").is_file());

        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "com.test.one");

        let enabled = store.enabled().unwrap();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].manifest.id, "com.test.one");
    }

    #[test]
    fn install_rejects_missing_manifest_and_bad_manifest() {
        let (_t, store) = store("bad");

        // 完全没有 manifest.json
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("index.js", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let err = store.install(&buf).unwrap_err().to_string();
        assert!(err.contains("manifest.json"), "应报缺少 manifest: {err}");

        // manifest 非法（工具名为大写）
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("manifest.json", opts).unwrap();
            w.write_all(br#"{"id":"ok-id","name":"n","version":"1","tools":[{"name":"Bad","description":"d"}]}"#).unwrap();
            w.start_file("index.js", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let err = store.install(&buf).unwrap_err().to_string();
        assert!(err.contains("工具名"), "应报工具名非法: {err}");

        // 非 zip 数据
        assert!(store.install(b"definitely not a zip").is_err());
        assert!(store.install(b"").is_err());

        // 失败后不应留下任何目录
        assert_eq!(store.list().len(), 0, "失败的安装不应留下半成品");
    }

    #[test]
    fn install_rejects_entry_missing_inside_package() {
        let (_t, store) = store("noentry");
        // manifest 声明 entry 指向不存在的文件
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("manifest.json", opts).unwrap();
            w.write_all(br#"{"id":"ok-id","name":"n","version":"1","entry":"missing.js","tools":[{"name":"ab","description":"d"}]}"#).unwrap();
            w.finish().unwrap();
        }
        let err = store.install(&buf).unwrap_err().to_string();
        assert!(err.contains("入口文件"), "应报找不到入口: {err}");
        assert_eq!(store.list().len(), 0);
    }

    #[test]
    fn install_rejects_zip_slip_entry() {
        let (_t, store) = store("slip");
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("manifest.json", opts).unwrap();
            w.write_all(br#"{"id":"ok-id","name":"n","version":"1","tools":[{"name":"ab","description":"d"}]}"#).unwrap();
            w.start_file("index.js", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.start_file("../../evil.js", opts).unwrap();
            w.write_all(b"pwned").unwrap();
            w.finish().unwrap();
        }
        let err = store.install(&buf).unwrap_err().to_string();
        assert!(err.contains("非法路径"), "应报非法路径: {err}");

        // 关键断言：逃逸目标没有被写出来
        let evil = store.root().join("..").join("..").join("evil.js");
        assert!(!evil.exists(), "zip-slip 逃逸文件不应存在");
        assert_eq!(store.list().len(), 0);
    }

    #[test]
    fn enable_disable_and_uninstall() {
        let (_t, store) = store("lifecycle");
        store.install(&make_plugin_zip("com.test.two", &[])).unwrap();

        store.set_enabled("com.test.two", false).unwrap();
        assert!(store.list()[0].enabled == false);
        assert!(
            store.enabled().unwrap().is_empty(),
            "停用后不应出现在运行时清单里"
        );

        store.set_enabled("com.test.two", true).unwrap();
        assert_eq!(store.enabled().unwrap().len(), 1);

        store.uninstall("com.test.two").unwrap();
        assert!(store.list().is_empty());
        assert!(!store.root().join("com.test.two").exists());

        // 卸载不存在的插件应报错而不是静默成功
        assert!(store.uninstall("com.test.two").is_err());
        assert!(store.set_enabled("com.test.two", true).is_err());
    }

    #[test]
    fn state_survives_reload_and_corrupt_state_is_tolerated() {
        let (_t, store) = store("state");
        store.install(&make_plugin_zip("com.test.three", &[])).unwrap();
        store.set_enabled("com.test.three", false).unwrap();

        // 重建 store（模拟重启）后状态仍在
        let reopened = PluginStore::new(&AgentDirs::from_root(
            store.root().parent().unwrap().to_path_buf(),
            "deepharness",
        ));
        assert!(reopened.list()[0].enabled == false, "启停状态应当持久化");

        // 状态文件损坏 → 按默认（启用）处理，且不 panic
        std::fs::write(store.root().join(STATE_FILE), b"{ not json").unwrap();
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled, "状态损坏时按默认启用处理");
    }

    #[test]
    fn invalid_plugin_id_rejected_before_touching_disk() {
        let (_t, store) = store("badid");
        assert!(store.set_enabled("../evil", true).is_err());
        assert!(store.uninstall("a/b").is_err());
        assert!(store.set_enabled("", true).is_err());
    }

    #[test]
    fn upgrade_replaces_previous_version() {
        let (_t, store) = store("upgrade");
        store.install(&make_plugin_zip("com.test.up", &[("old.txt", "v1")])).unwrap();
        assert!(store.root().join("com.test.up").join("old.txt").is_file());

        // 再装一次（不带 old.txt）→ 旧文件不应残留
        store.install(&make_plugin_zip("com.test.up", &[])).unwrap();
        assert!(
            !store.root().join("com.test.up").join("old.txt").exists(),
            "升级应替换整个目录，而不是叠加"
        );
        // 临时目录也不应残留
        assert!(!store.root().join(".staging-com.test.up").exists());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn broken_manifest_is_listed_not_hidden() {
        let (_t, store) = store("broken");
        let dir = store.root().join("com.test.broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), b"{ nope").unwrap();

        let list = store.list();
        assert_eq!(list.len(), 1, "损坏的插件应被列出以便 UI 提示");
        assert!(!list[0].valid);
        assert!(list[0].error.is_some());
        assert!(store.enabled().unwrap().is_empty(), "损坏插件不进入运行时");
        // 但目录名要被保留为 id
        assert_eq!(list[0].id, "com.test.broken");
    }
}

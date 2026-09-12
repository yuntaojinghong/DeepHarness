//! `.dph-plugin` 插件系统（路线 A：只服务 DeepHarness 自研 Agent 循环）。
//!
//! ## 包格式
//!
//! 一个 `.dph-plugin` 就是一个 zip 包，根目录下必须有：
//!
//! ```text
//! manifest.json   # 插件元信息与工具声明
//! index.js        # 入口（默认 index.js，可用 manifest.entry 改名）
//! ```
//!
//! `index.js` 导出若干工具函数：
//!
//! ```js
//! module.exports = {
//!   tools: {
//!     async hello_greet(args, ctx) {
//!       const text = await ctx.readFile(args.path); // 经宿主回到 Rust 白名单
//!       return { message: `你好 ${args.who}`, size: text.length };
//!     },
//!   },
//! };
//! ```
//!
//! ## 安全模型
//!
//! 插件代码是**不受信任的第三方 JS**，所以做了三重隔离：
//!
//! 1. **进程隔离**：插件在自己的 `node` 子进程里执行。崩溃、死循环、
//!    内存爆炸只影响那一次调用（超时即杀进程），拖不垮 Worker。
//! 2. **文件系统隔离**：子进程以
//!    `--permission --allow-fs-read=<插件目录>` 启动。实测（随包 Node
//!    22.22.2）：越界读报 `ERR_ACCESS_DENIED`，任何写操作一律被拒，
//!    因此插件**无法直接触碰用户磁盘**。
//! 3. **白名单代理**：插件要碰真实文件，只能通过 `ctx.readFile` /
//!    `ctx.writeFile` / `ctx.listDir` 回到 Rust 侧，由 `PermissionStore`
//!    按该 Agent 的白名单**逐次**校验——与内置工具走完全同一道闸门。
//!
//! ## 为什么不用 dsh 的 cordis 插件
//!
//! dsh 的插件体系是 cordis（依赖注入框架），其网页端整个前端都是插件
//! 拼装的；而本项目的自研 React 界面**不经过 dsh**（直连 chat
//! completions）。两者互不通用，所以本格式只服务自研循环，**不与
//! cordis 生态兼容**——这是明确的分工，不是待补的缺口。

pub mod commands;
pub mod host;
pub mod store;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;

pub use host::{HOST_SHIM_SOURCE, Invocation, PluginHost, materialize_shim};
pub use store::{MAX_ARCHIVE_BYTES, PluginRecord, PluginStore};

/// 单个插件可声明的工具数上限。
pub const MAX_TOOLS_PER_PLUGIN: usize = 16;

/// 插件 id 长度上限。
const MAX_ID_LEN: usize = 64;

/// 默认入口文件名。
fn default_entry() -> String {
    "index.js".to_string()
}

/// manifest 里声明的单个工具。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestTool {
    /// 工具名。必须是小写 snake_case，且不能与内置工具重名。
    pub name: String,
    /// 给模型看的功能描述。
    pub description: String,
    /// 参数签名的文字说明（如 `{ "path": string }`），进入规划器提示词。
    #[serde(default)]
    pub args_schema: String,
}

/// `manifest.json` 的结构。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    /// 全局唯一 id（建议反向域名，如 `com.example.tools`）。同时用作目录名。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 版本号。
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: String,
    /// 入口文件（相对插件目录）。默认 `index.js`。
    #[serde(default = "default_entry")]
    pub entry: String,
    /// 该插件提供的工具。
    pub tools: Vec<ManifestTool>,
}

impl PluginManifest {
    /// 校验 manifest 的每一项约束；返回可读的中文错误。
    pub fn validate(&self) -> AppResult<()> {
        validate_plugin_id(&self.id)?;

        if self.name.trim().is_empty() {
            return Err(AppError::Other("manifest.name 不能为空".to_string()));
        }
        if self.name.chars().count() > 64 {
            return Err(AppError::Other("manifest.name 过长（上限 64 字符）".to_string()));
        }
        validate_version(&self.version)?;
        validate_entry(&self.entry)?;

        if self.tools.is_empty() {
            return Err(AppError::Other("manifest.tools 不能为空".to_string()));
        }
        if self.tools.len() > MAX_TOOLS_PER_PLUGIN {
            return Err(AppError::Other(format!(
                "manifest.tools 过多（{} 个，上限 {MAX_TOOLS_PER_PLUGIN} 个）",
                self.tools.len()
            )));
        }

        let mut seen: Vec<&str> = Vec::with_capacity(self.tools.len());
        for tool in &self.tools {
            validate_tool_name(&tool.name)?;
            if tool.description.trim().is_empty() {
                return Err(AppError::Other(format!(
                    "工具 `{}` 缺少 description",
                    tool.name
                )));
            }
            if seen.contains(&tool.name.as_str()) {
                return Err(AppError::Other(format!(
                    "工具名 `{}` 在同一个插件内重复",
                    tool.name
                )));
            }
            seen.push(&tool.name);
        }
        Ok(())
    }
}

/// 校验插件 id。
///
/// 允许 `[a-z0-9]`、`-`、`_`、`.`，必须以字母或数字开头结尾，禁止
/// 连续的点（挡住 `..`）。id 会被当作目录名使用，所以不允许任何
/// 路径分隔符或盘符。
pub fn validate_plugin_id(id: &str) -> AppResult<()> {
    let cjk_len = id.chars().count();
    if cjk_len == 0 || cjk_len > MAX_ID_LEN {
        return Err(AppError::Other(format!(
            "插件 id 长度必须在 1..={MAX_ID_LEN} 之间"
        )));
    }
    let first = id.chars().next().unwrap();
    let last = id.chars().last().unwrap();
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(AppError::Other(
            "插件 id 必须以小写字母或数字开头".to_string(),
        ));
    }
    if !last.is_ascii_lowercase() && !last.is_ascii_digit() {
        return Err(AppError::Other(
            "插件 id 必须以小写字母或数字结尾".to_string(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
    {
        return Err(AppError::Other(format!(
            "插件 id `{id}` 含非法字符（只允许小写字母、数字、- _ .）"
        )));
    }
    if id.contains("..") {
        return Err(AppError::Other(format!(
            "插件 id `{id}` 含连续的点（可能是路径穿越）"
        )));
    }
    Ok(())
}

/// 校验版本号：非空、含数字、字符集受限。
fn validate_version(version: &str) -> AppResult<()> {
    if version.is_empty() || version.chars().count() > 32 {
        return Err(AppError::Other(
            "manifest.version 长度必须在 1..=32 之间".to_string(),
        ));
    }
    if !version.chars().any(|c| c.is_ascii_digit()) {
        return Err(AppError::Other(
            "manifest.version 至少要包含一个数字".to_string(),
        ));
    }
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
    {
        return Err(AppError::Other(
            "manifest.version 只允许字母、数字与 . - +".to_string(),
        ));
    }
    Ok(())
}

/// 校验入口文件：必须是插件目录内的相对路径。
fn validate_entry(entry: &str) -> AppResult<()> {
    if entry.trim().is_empty() {
        return Err(AppError::Other("manifest.entry 不能为空".to_string()));
    }
    if entry.len() > 128 {
        return Err(AppError::Other("manifest.entry 过长".to_string()));
    }
    let lowered = entry.to_ascii_lowercase();
    if !(lowered.ends_with(".js") || lowered.ends_with(".mjs") || lowered.ends_with(".cjs")) {
        return Err(AppError::Other(
            "manifest.entry 必须是 .js / .mjs / .cjs 文件".to_string(),
        ));
    }
    // 绝不能是绝对路径、盘符或向上跳目录
    if entry.starts_with('/') || entry.starts_with('\\') || entry.contains(':') {
        return Err(AppError::Other(format!(
            "manifest.entry `{entry}` 必须是插件目录内的相对路径"
        )));
    }
    if !crate::plugins::store::is_safe_relative(entry) {
        return Err(AppError::Other(format!(
            "manifest.entry `{entry}` 含非法路径片段"
        )));
    }
    Ok(())
}

/// 校验工具名：小写 snake_case，2..=48 字符。
///
/// 与内置工具同名会被 `tool_catalog` 拦下（内置优先），但这里先做
/// 格式约束，避免插件声明出前端与提示词都无法正常处理的怪名字。
pub fn validate_tool_name(name: &str) -> AppResult<()> {
    let len = name.chars().count();
    if !(2..=48).contains(&len) {
        return Err(AppError::Other(format!(
            "工具名 `{name}` 长度必须在 2..=48 之间"
        )));
    }
    if !name.chars().next().unwrap().is_ascii_lowercase() {
        return Err(AppError::Other(format!(
            "工具名 `{name}` 必须以小写字母开头"
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AppError::Other(format!(
            "工具名 `{name}` 只允许小写字母、数字与下划线"
        )));
    }
    Ok(())
}

/// 运行期的一个插件工具（已通过校验、且所属插件已启用）。
#[derive(Debug, Clone, PartialEq)]
pub struct PluginTool {
    pub tool: ManifestTool,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_dir: PathBuf,
    pub entry: String,
}

/// 插件运行时：把「已启用的插件」解析成一张可查询的工具表。
///
/// 由 Worker 在 `configure` 时按 Agent 的插件目录构建一次；磁盘上的
/// 插件发生变化（安装 / 启停 / 卸载）后由主进程重新 configure 生效。
pub struct PluginRuntime {
    host: PluginHost,
    tools: Vec<PluginTool>,
}

impl PluginRuntime {
    /// 从某个 Agent 的插件目录构建运行时。
    ///
    /// `store` 已经过 `AgentDirs::plugins` 隔离，天然只含本 Agent 的插件。
    pub fn load(store: &PluginStore, host: PluginHost) -> AppResult<Self> {
        let mut tools = Vec::new();
        for installed in store.enabled()? {
            let manifest = &installed.manifest;
            for tool in &manifest.tools {
                tools.push(PluginTool {
                    tool: tool.clone(),
                    plugin_id: manifest.id.clone(),
                    plugin_name: manifest.name.clone(),
                    plugin_dir: installed.dir.clone(),
                    entry: manifest.entry.clone(),
                });
            }
        }
        Ok(Self { host, tools })
    }

    /// 空运行时（未配置插件能力时使用）。
    pub fn empty(host: PluginHost) -> Self {
        Self {
            host,
            tools: Vec::new(),
        }
    }

    /// 当前可用的插件工具。
    pub fn tools(&self) -> &[PluginTool] {
        &self.tools
    }

    /// 是否没有任何插件工具。
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 按名字查找插件工具。
    ///
    /// 不同插件声明同名工具时，**先注册者优先**（按插件 id 排序稳定），
    /// 保证同一份磁盘状态每次解析结果一致。
    pub fn find(&self, name: &str) -> Option<&PluginTool> {
        self.tools.iter().find(|t| t.tool.name == name)
    }

    /// 执行宿主（工具分派需要用它来跑插件）。
    pub fn host(&self) -> &PluginHost {
        &self.host
    }

    /// 宿主可执行文件与 shim 路径（用于诊断信息）。
    pub fn host_paths(&self) -> (&std::path::Path, &std::path::Path) {
        (self.host.node(), self.host.shim())
    }
}

#[cfg(test)]
impl PluginRuntime {
    /// 测试专用构造：直接给一张工具表，不读磁盘。
    pub fn for_test(host: PluginHost, tools: Vec<PluginTool>) -> Self {
        Self { host, tools }
    }
}

/// 便捷入口：按 Agent 目录构建插件运行时。
pub fn runtime_for(dirs: &AgentDirs, node: PathBuf, shim: PathBuf) -> AppResult<PluginRuntime> {
    let store = PluginStore::new(dirs);
    PluginRuntime::load(&store, PluginHost::new(node, shim))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            name: "示例".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            homepage: String::new(),
            entry: default_entry(),
            tools: vec![ManifestTool {
                name: "hello_greet".to_string(),
                description: "打招呼".to_string(),
                args_schema: r#"{ "who": string }"#.to_string(),
            }],
        }
    }

    #[test]
    fn valid_manifest_passes() {
        assert!(sample("com.example.hello").validate().is_ok());
        assert!(sample("a1").validate().is_ok());
        assert!(sample("tools-2").validate().is_ok());
    }

    #[test]
    fn plugin_id_rules() {
        // 合法
        for id in ["a", "a1", "com.example.x", "my-tools", "my_tools"] {
            assert!(validate_plugin_id(id).is_ok(), "{id} 应当合法");
        }
        // 非法：大小写、起止符号、路径穿越、路径分隔符、空
        for id in [
            "",
            "A",
            "com.Example",
            "-lead",
            "trail-",
            "has..dots",
            "../evil",
            "a/b",
            "a\\b",
            "C:evil",
            "dot.",
            ".dot",
            "有中文",
        ] {
            assert!(validate_plugin_id(id).is_err(), "{id} 应当被拒绝");
        }
    }

    #[test]
    fn plugin_id_length_bounds() {
        let long = "a".repeat(MAX_ID_LEN);
        assert!(validate_plugin_id(&long).is_ok());
        let too_long = "a".repeat(MAX_ID_LEN + 1);
        assert!(validate_plugin_id(&too_long).is_err());
    }

    #[test]
    fn tool_name_rules() {
        for name in ["ab", "read_2", "hello_greet", "trailing_"] {
            assert!(validate_tool_name(name).is_ok(), "{name} 应当合法");
        }
        for name in ["", "a", "A_b", "has-dash", "has space", "有中文", "1abc"] {
            assert!(validate_tool_name(name).is_err(), "{name} 应当被拒绝");
        }
    }

    #[test]
    fn version_rules() {
        for v in ["1", "1.0.0", "v1.0.0-rc.1", "1.0.0+build"] {
            assert!(validate_version(v).is_ok(), "{v} 应当合法");
        }
        for v in ["", "abc", "1.0 0", "1.0/2"] {
            assert!(validate_version(v).is_err(), "{v} 应当被拒绝");
        }
    }

    #[test]
    fn entry_rules() {
        for e in ["index.js", "dist/main.js", "a/b/c.mjs", "index.cjs"] {
            assert!(validate_entry(e).is_ok(), "{e} 应当合法");
        }
        for e in [
            "",
            "index.txt",
            "/abs/index.js",
            "C:/x/index.js",
            "../escape.js",
            "a/../../b.js",
        ] {
            assert!(validate_entry(e).is_err(), "{e} 应当被拒绝");
        }
    }

    #[test]
    fn manifest_rejects_empty_and_duplicate_tools() {
        let mut m = sample("dup");
        m.tools.clear();
        assert!(m.validate().is_err(), "空工具表应被拒绝");

        let mut m = sample("dup2");
        let t = m.tools[0].clone();
        m.tools.push(t);
        let err = m.validate().unwrap_err().to_string();
        assert!(err.contains("重复"), "应报重复: {err}");
    }

    #[test]
    fn manifest_rejects_too_many_tools() {
        let mut m = sample("many");
        m.tools.clear();
        for i in 0..=MAX_TOOLS_PER_PLUGIN {
            m.tools.push(ManifestTool {
                name: format!("tool_{i}"),
                description: "x".to_string(),
                args_schema: String::new(),
            });
        }
        assert!(m.validate().is_err());
    }

    #[test]
    fn manifest_rejects_unknown_fields_and_empty_name() {
        let raw = r#"{"id":"ok-id","name":"n","version":"1","tools":[{"name":"ab","description":"d","oops":1}],"extra":true}"#;
        assert!(
            serde_json::from_str::<PluginManifest>(raw).is_err(),
            "未知字段应当报错而不是被静默忽略"
        );

        let mut m = sample("blank-name");
        m.name = "   ".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn manifest_serde_roundtrip() {
        let m = sample("round-trip");
        let json = serde_json::to_string(&m).unwrap();
        let back: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn manifest_entry_defaults_to_index_js() {
        let raw = r#"{"id":"def","name":"n","version":"1","tools":[{"name":"ab","description":"d"}]}"#;
        let m: PluginManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.entry, "index.js");
        assert!(m.validate().is_ok());
    }
}

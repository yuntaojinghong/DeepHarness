//! DeepHarness 应用入口。
//!
//! 模块职责：
//! - `permissions` / `fs_ops` / `paths`：文件访问权限层；
//! - `sessions`：每 Agent 独立会话存储；
//! - `agents`：Agent 运行时隔离层（AgentRuntime / 注册表 / Worker 协议）；
//! - `logging`：tracing 日志初始化；
//! - 本文件：Tauri Builder 组装与应用级 KV 存储命令。
//!
//! 安全说明：旧版曾暴露无任何限制的 `run_command` 命令，本版本已移除；
//! Agent 的全部文件操作必须经由 `fs_ops` 中带白名单校验的命令完成。

pub mod agents;
mod error;
mod fs_ops;
mod logging;
pub mod native;
mod open_url;
pub mod paths;
pub mod permissions;
mod process;
mod sessions;

use std::path::PathBuf;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

use agents::registry::AgentRegistry;
use agents::AgentStatus;
use fs_ops::BaseDir;
use paths::AgentDirs;
use permissions::PermissionStore;
use sessions::SessionStore;

#[derive(Serialize, Clone)]
struct RuntimeInfo {
    installed: bool,
    version: Option<String>,
    path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvStatus {
    python: Option<RuntimeInfo>,
    node: Option<RuntimeInfo>,
    git: Option<RuntimeInfo>,
    os: String,
    arch: String,
    data_dir: String,
    portable: bool,
}

fn detect(cmd: &str, version_args: &[&str]) -> RuntimeInfo {
    let path = crate::process::command("where")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|s| s.trim().to_string())
        });

    let version = crate::process::command(cmd)
        .args(version_args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if out.is_empty() {
                String::from_utf8_lossy(&o.stderr).trim().to_string()
            } else {
                out
            }
        })
        .filter(|s| !s.is_empty());

    RuntimeInfo {
        installed: path.is_some(),
        version,
        path,
    }
}

fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("portable.flag").exists()))
        .unwrap_or(false)
}

fn portable_data_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.flag").exists() {
                return dir.join("data");
            }
        }
    }
    app.path()
        .app_data_dir()
        .unwrap_or_default()
        .join("data")
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

#[tauri::command]
fn check_env(app: tauri::AppHandle) -> EnvStatus {
    EnvStatus {
        python: Some(detect("python", &["--version"])),
        node: Some(detect("node", &["--version"])),
        git: Some(detect("git", &["--version"])),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        data_dir: portable_data_dir(&app).display().to_string(),
        portable: is_portable(),
    }
}

/// 应用级 KV 存储（键已做白名单字符过滤，仅用于应用自身设置；
/// Agent 级数据一律走各自的隔离目录，不经过这里）。
#[tauri::command]
fn read_store(app: tauri::AppHandle, key: String) -> Result<String, String> {
    let path = portable_data_dir(&app).join(format!("{}.json", sanitize_key(&key)));
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn write_store(app: tauri::AppHandle, key: String, value: String) -> Result<(), String> {
    let dir = portable_data_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", sanitize_key(&key)));
    std::fs::write(&path, value).map_err(|e| e.to_string())
}

#[tauri::command]
fn notify(app: tauri::AppHandle, title: String, body: String) {
    let _ = app.notification().builder().title(title).body(body).show();
}

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    url: String,
}

#[tauri::command]
fn check_update() -> Option<UpdateInfo> {
    let output = crate::process::command("curl")
        .args([
            "--ssl-no-revoke",
            "-s",
            "--max-time",
            "8",
            "https://api.github.com/repos/yuntaojinghong/DeepHarness/releases/latest",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let version = json.get("tag_name")?.as_str()?.to_string();
    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://github.com/yuntaojinghong/DeepHarness/releases")
        .to_string();
    Some(UpdateInfo { version, url })
}

/// 组装三个 Agent 的注册表（启动期调用一次）。
///
/// 返回注册表与 DeepHarness Native 运行时的共享句柄：注册表做统一
/// 启停/状态管理，句柄供 DeepHarness 专属命令直接访问（避免长任务
/// 阻塞注册表全局锁）。
fn build_agent_registry(
    app: &tauri::AppHandle,
    data_dir: &std::path::Path,
) -> error::AppResult<(
    AgentRegistry,
    std::sync::Arc<agents::native::NativeAgentRuntime>,
)> {
    agents::registry::ensure_all_agent_dirs(data_dir)?;
    let registry = AgentRegistry::default();

    // 1) DeepSeek Harness：内置 dsh CLI（node 子进程）
    let dsh_dirs = AgentDirs::from_root(paths::agent_root(data_dir, "deepseek-harness"), "deepseek-harness");
    let node = agents::dsh::DshRuntime::find_resource(app, "node/node.exe");
    let dsh_bin = agents::dsh::DshRuntime::find_resource(app, "dsh/node_modules/@deepseek-ai/dsh/lib/bin.js");
    registry.register(std::sync::Arc::new(agents::dsh::DshRuntime::new(dsh_dirs, node, dsh_bin)))?;

    // 2) Codex：系统 codex CLI（按需拉起，CODEX_HOME 隔离）
    let codex_dirs = AgentDirs::from_root(paths::agent_root(data_dir, "codex"), "codex");
    registry.register(std::sync::Arc::new(agents::codex::CodexRuntime::new(
        codex_dirs,
        Some(app.clone()),
    )))?;

    // 3) DeepHarness：自研 Agent（自我重执行 Sidecar Worker）
    let native_dirs = AgentDirs::from_root(paths::agent_root(data_dir, "deepharness"), "deepharness");
    let worker_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("DeepHarness"));
    let native = std::sync::Arc::new(agents::native::NativeAgentRuntime::new(
        native_dirs,
        worker_exe,
        data_dir.to_path_buf(),
    ));
    registry.register(native.clone())?;

    Ok((registry, native))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 应用数据根目录：<data>/data（便携模式）或 app_data_dir/data
            let data_dir = portable_data_dir(app.handle());
            std::fs::create_dir_all(&data_dir)?;

            // 日志：<data>/logs
            logging::init(&data_dir.join("logs"));
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "DeepHarness 启动");

            // 权限层：每 Agent 独立白名单
            let perms = PermissionStore::new(data_dir.clone());
            app.manage(perms);
            app.manage(BaseDir(data_dir.clone()));

            // 会话存储：每 Agent 独立
            let sessions = SessionStore::new(data_dir.clone());
            app.manage(sessions);

            // Agent 注册表：三个相互隔离的运行时
            let (registry, native) = build_agent_registry(app.handle(), &data_dir)
                .map_err(|e| format!("初始化 Agent 注册表失败: {e}"))?;
            app.manage(registry);
            app.manage(agents::registry::NativeAgentHandle(native));

            let show_i = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("DeepHarness")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_env,
            read_store,
            write_store,
            notify,
            check_update,
            // 权限层命令
            fs_ops::check_access,
            fs_ops::grant_access,
            fs_ops::revoke_access,
            fs_ops::list_grants,
            fs_ops::read_text_file,
            fs_ops::read_file_base64,
            fs_ops::write_text_file,
            fs_ops::list_directory,
            fs_ops::create_directory,
            fs_ops::delete_path,
            fs_ops::agent_dirs_info,
            // Agent 隔离层命令
            agents::registry::agent_list,
            agents::registry::agent_start,
            agents::registry::agent_stop,
            agents::registry::agent_status,
            agents::registry::agent_open_web_ui,
            // 系统级能力
            open_url::open_external_url,
            // DeepHarness Native Agent 命令
            agents::registry::deepharness_configure,
            agents::registry::deepharness_get_config,
            agents::registry::deepharness_status,
            agents::registry::deepharness_run_task,
            agents::registry::deepharness_plan,
            agents::registry::deepharness_remember,
            agents::registry::deepharness_recall,
            agents::registry::deepharness_forget,
            agents::registry::deepharness_memory_stats,
            // 会话命令
            agents::registry::session_list,
            agents::registry::session_get,
            agents::registry::session_create,
            agents::registry::session_append_message,
            agents::registry::session_rename,
            agents::registry::session_delete,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// AgentStatus 在本文件中的类型引用（overview 序列化经 registry 模块导出）。
#[allow(dead_code)]
fn _assert_status_used(_: AgentStatus) {}

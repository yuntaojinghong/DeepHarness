use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

struct DshProcess(Mutex<Option<std::process::Child>>);

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
    let path = Command::new("where")
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

    let version = Command::new(cmd)
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
struct RunResult {
    code: i32,
    stdout: String,
    stderr: String,
}

#[tauri::command]
fn run_command(command: String, args: Vec<String>, cwd: Option<String>) -> RunResult {
    let mut cmd = Command::new(&command);
    cmd.args(&args);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    match cmd.output() {
        Ok(o) => RunResult {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).to_string(),
            stderr: String::from_utf8_lossy(&o.stderr).to_string(),
        },
        Err(e) => RunResult {
            code: -1,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

#[tauri::command]
fn list_dir(path: String) -> Vec<DirEntry> {
    let mut out: Vec<DirEntry> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&path) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let size = if is_dir {
                0
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            };
            out.push(DirEntry {
                name,
                is_dir,
                size,
            });
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn find_resource(app: &tauri::AppHandle, relative: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    // 优先：exe 目录（NSIS 安装版资源在 exe 目录的 _up_/resources）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("_up_").join("resources").join(relative));
            candidates.push(dir.join("resources").join(relative));
            candidates.push(dir.join(relative));
        }
    }
    // 兜底：resource_dir
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join(relative));
        candidates.push(res.join("resources").join(relative));
        candidates.push(res.join("_up_").join("resources").join(relative));
    }
    // 必须是真实文件（is_file 排除 "C:" 这类目录路径）
    candidates.into_iter().find(|p| p.is_file())
}

fn kill_port_3080() {
    if let Ok(out) = Command::new("netstat").args(["-ano"]).output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if line.contains(":3080") && line.to_ascii_uppercase().contains("LISTENING") {
                if let Some(pid) = line.split_whitespace().last() {
                    let _ = Command::new("taskkill")
                        .args(["/F", "/PID", pid])
                        .output();
                }
            }
        }
    }
}

#[tauri::command]
fn start_dsh(app: tauri::AppHandle, state: State<DshProcess>) -> Result<String, String> {
    let node = find_resource(&app, "node/node.exe").ok_or_else(|| "内置 Node 缺失".to_string())?;
    let dsh_bin = find_resource(&app, "dsh/node_modules/@deepseek-ai/dsh/lib/bin.js")
        .ok_or_else(|| "dsh 缺失".to_string())?;

    if let Some(mut c) = state.0.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    // 清理占用 3080 的残留进程（避免端口冲突导致 dsh 退出）
    kill_port_3080();

    let data_dir = portable_data_dir(&app);
    let log_path = data_dir.join("dsh.log");
    if let Some(p) = log_path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let log_file = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;

    let child = Command::new(&node)
        .env("DSH_HOME", &data_dir)
        .arg(&dsh_bin)
        .arg("web")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("3080")
        .arg("--no-open")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file))
        .spawn()
        .map_err(|e| format!("启动 dsh 失败: {e}"))?;

    *state.0.lock().unwrap() = Some(child);

    // 轮询 3080 端口，等待 dsh 就绪（最多 90 秒，每 500ms 一次）
    for _ in 0..180 {
        if std::net::TcpStream::connect(("127.0.0.1", 3080)).is_ok() {
            return Ok("dsh ready".into());
        }
        let mut guard = state.0.lock().unwrap();
        let mut exited = false;
        if let Some(c) = guard.as_mut() {
            if let Ok(Some(_status)) = c.try_wait() {
                exited = true;
            }
        }
        drop(guard);
        if exited {
            let mut full = String::new();
            if let Ok(mut f) = std::fs::File::open(&log_path) {
                use std::io::Read;
                let _ = f.read_to_string(&mut full);
            }
            let truncated: String = full.trim().chars().take(800).collect();
            return Err(format!("dsh 启动失败：{truncated}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err("dsh 启动超时（90 秒）".into())
}

#[tauri::command]
fn stop_dsh(state: State<DshProcess>) {
    if let Some(mut c) = state.0.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[tauri::command]
fn dsh_status(state: State<DshProcess>) -> bool {
    state.0.lock().unwrap().is_some()
}

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    url: String,
}

#[tauri::command]
fn check_update() -> Option<UpdateInfo> {
    let output = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "8",
            "https://api.github.com/repos/yuntaojinghong/DeepSeek-Harness-Desktop/releases/latest",
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
        .unwrap_or("https://github.com/yuntaojinghong/DeepSeek-Harness-Desktop/releases")
        .to_string();
    Some(UpdateInfo { version, url })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(DshProcess(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            check_env,
            run_command,
            list_dir,
            read_store,
            write_store,
            notify,
            start_dsh,
            stop_dsh,
            dsh_status,
            check_update
        ])
        .setup(|app| {
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
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

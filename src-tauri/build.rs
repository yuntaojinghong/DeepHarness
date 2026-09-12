//! 构建脚本：为 Windows 产物统一嵌入应用清单。
//!
//! 背景：`tao` 静态导入 `comctl32.dll!TaskDialogIndirect`，该函数只存在于
//! Common-Controls v6（WinSxS 侧装程序集）。没有清单时 loader 会绑定
//! `System32\comctl32.dll`（5.82），入口点解析失败 →
//! `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`，一行测试都不会执行。
//!
//! tauri-build 默认只把清单嵌进 **bin** 目标（内部走 `embed-resource` 的
//! `rustc-link-arg-bins`），`cargo test` 链接出的可执行文件完全没有清单，
//! 于是所有单元测试都会在加载期直接失败。
//!
//! 处理办法：关闭 tauri-build 的默认清单，改由本脚本对**所有链接目标**
//! 追加同一份 `windows-app.manifest`。这样 bin 与测试目标共用唯一一份
//! 清单来源，既补齐了测试目标，也不会出现「两套清单叠加」导致的
//! `.rsrc` 合并告警或清单内容漂移。
//!
//! 不能改用 `cargo:rustc-link-arg-tests`：该变体按 target kind 匹配，
//! 只作用于 `tests/` 集成测试目标，lib 的单元测试（`cargo test --lib`）
//! 拿不到清单——而单元测试才是本项目测试的主体。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    // 关闭 tauri-winres 的默认清单：清单由本脚本统一提供。
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attrs).expect("tauri-build 构建失败");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let manifest = manifest_dir.join("windows-app.manifest");
    if !manifest.is_file() {
        panic!(
            "窗口清单缺失，Windows 产物将无法加载：{}",
            manifest.display()
        );
    }
    println!("cargo:rerun-if-changed=windows-app.manifest");

    if target.contains("msvc") {
        // MSVC 链接器直接接收清单文件。
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        return;
    }

    // GNU（MinGW）工具链没有 /MANIFESTINPUT，改用 windres 把清单编译成
    // 资源对象再交给链接器。GNU 仅用于本机开发校验，正式产物为 MSVC。
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let rc = out_dir.join("app-manifest.rc");
    let obj = out_dir.join("app-manifest.o");
    // 24 = RT_MANIFEST，1 = CREATEPROCESS_MANIFEST_RESOURCE_ID。
    if let Err(e) = std::fs::write(&rc, "1 24 \"windows-app.manifest\"\n") {
        panic!("写入清单资源脚本失败：{e}");
    }

    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".to_string());
    let status = Command::new(&windres)
        .arg("--include-dir")
        .arg(&manifest_dir)
        .arg("--input")
        .arg(&rc)
        .arg("--output")
        .arg(&obj)
        .arg("--output-format=coff")
        .status();

    match status {
        Ok(s) if s.success() && obj.is_file() => {
            println!("cargo:rustc-link-arg={}", obj.display());
        }
        Ok(s) => panic!(
            "{windres} 未能生成清单资源对象（退出码 {:?}），Windows 产物将无法加载",
            s.code()
        ),
        Err(e) => panic!("无法执行 {windres}：{e}"),
    }
}

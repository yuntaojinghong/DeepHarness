fn main() {
    tauri_build::build();

    // 测试二进制（cargo test）需要嵌入应用清单：tao 静态导入
    // comctl32.dll!TaskDialogIndirect（仅 Common-Controls v6 导出）。
    // 没有清单时 loader 找到 comctl32 5.82 → 0xc0000139，测试全军覆没。
    // bin 目标由 tauri-build 自行处理，这里只补 tests 目标。
    // 注意：/MANIFEST:EMBED 与 /MANIFESTINPUT 必须成对且按此顺序传递。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") && target.contains("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("windows-app.manifest");
        if manifest.is_file() {
            println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
        } else {
            panic!("windows-app.manifest 缺失: {}", manifest.display());
        }
    }
}

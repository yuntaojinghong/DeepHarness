fn main() {
    tauri_build::build();

    // 测试二进制（cargo test，lib 的 unittests）需要嵌入应用清单：tao 静态
    // 导入 comctl32.dll!TaskDialogIndirect，该函数仅存在于 Common-Controls v6
    // （WinSxS 侧装程序集）。没有清单时 loader 绑定 System32 的 comctl32 5.82
    // → STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)，一行测试都不会执行。
    //
    // 为什么用 `rustc-link-arg` 而不是 `rustc-link-arg-tests`：
    // 后者要求包内存在 test 类型的 target；本包只有 lib（其 unittests 属于
    // lib target 的 test 配置）与 bin，cargo 会直接报
    // "The package ... does not have a test target" 并中断构建。
    //
    // 为什么不会与 bin 冲突：bin 目标由 tauri-build 经 tauri-winres 以资源
    // 方式嵌入清单（本身已含 Common-Controls v6 依赖），再叠加
    // /MANIFESTINPUT 会产生重复依赖。CI 因此只以 `cargo test --lib` 链接
    // 测试目标，不构建 bin 目标；本地非 msvc 工具链（GNU）不走此分支。
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") && target.contains("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("windows-app.manifest");
        if manifest.is_file() {
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
            println!("cargo:rerun-if-changed=windows-app.manifest");
        } else {
            panic!("windows-app.manifest 缺失: {}", manifest.display());
        }
    }
}

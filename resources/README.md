# resources 目录

这里放**随包资源**：Node 便携版（`node/node.exe`）与 dsh 依赖树
（`dsh/node_modules/@deepseek-ai/dsh/lib/bin.js`）。

它们体积很大（合计约 100MB），因此**不入库**，由脚本按版本下载安装：

```bash
bash scripts/setup-resources.sh          # 按 package.json 里的版本安装
bash scripts/setup-resources.sh --force  # 强制重装
```

## 为什么这里必须保留一个文件

`src-tauri/tauri.conf.json` 的 `bundle.resources` 指向 `../resources/**/*`，
而 **tauri-build 在编译期（build.rs）就会展开并校验这个 glob**。
目录被完全忽略掉、一个文件都不匹配时，编译会直接失败：

```
tauri-build 构建失败: glob pattern ../resources/**/* path not found or didn't match any files.
```

也就是说 `cargo check` / `cargo test` / `cargo tauri build` 全都跑不起来。
本文件就是为此存在的占位——它保证 glob 至少匹配到一个文件，
让**没装资源的环境也能正常编译**，同时它也会（无害地）被打进安装包。

真正的资源由 CI 在 `tauri build` 之前调用脚本生成；
运行时的查找逻辑见 `src-tauri/src/agents/dsh.rs` 的 `find_resource`，
它会同时候选资源根目录、`resources/` 子目录与 exe 同级目录三种落点。

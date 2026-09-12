# Changelog

本项目的所有值得注意的变更都会记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)（SemVer 2.0.0）。

## [Unreleased]

### Added
- **阶段 4 · DeepHarness Native Agent 核心**：
  - 模型提供方抽象（`ModelProvider` trait）+ DeepSeek Chat Completions 实现，HTTP 走 Windows 原生 WinHTTP（系统 SChannel TLS，零新增编译负担）；
  - 多步规划器：目标 → 结构化 JSON 计划（工具步骤 / 纯回答步骤），非法输出带错误反馈自动重试一次，步骤数上限防失控；
  - 工具注册表：`read_text_file` / `write_text_file` / `list_directory` / `create_directory` / `delete_path`，全部经由权限白名单校验（Worker 直接读取主进程维护的 `permissions.json`，授权状态实时同步）；
  - 反思器：每步执行后由模型结构化评审（success / summary / shouldRetry / advice），模型不可用时退化为按执行层结果判定的保守策略，保证任务循环有确定性出口；
  - 任务编排（`TaskRunner`）：规划 → 逐步执行 → 反思 → 失败自动重试一次 → 总结（模型失败时拼接步骤摘要兜底）→ 任务事件写入长期记忆；
  - SQLite 长期记忆库（rusqlite bundled，免系统依赖）：fact / preference / event / reflection 四类记忆，标签、重要度与分词召回，Worker 协议新增 `remember` / `recall` / `forget` / `memory_stats`；
  - Worker 协议扩展：`configure`（注入模型参数与 Agent 目录）、`plan`、`run_task` 及全部记忆操作，未配置请求返回明确错误。
  - 主进程 IPC 命令层：`deepharness_configure` / `deepharness_get_config`（apiKey 打码）/ `deepharness_status` / `deepharness_run_task` / `deepharness_plan` / `deepharness_remember` / `deepharness_recall` / `deepharness_forget` / `deepharness_memory_stats`；模型配置持久化至 Agent 隔离目录（`config/agent_config.json`，不回显 Key），Worker 启动时自动注入已保存配置，未运行时命令自动拉起 Worker；注册表改为 `Arc` 持有并新增 `NativeAgentHandle` 托管状态，长任务不阻塞注册表全局锁。
- **跨模块集成测试**（`src-tauri/tests/public_api.rs`）：只经由 `deepharness_lib::…` 公开接口访问，校验规划器↔工具注册表、`AccessMode` 与 serde 名、记忆类型、反思 JSON 的 camelCase、配置持久化与三 Agent 目录隔离等跨模块契约。

### Changed
- Windows 构建链补齐：MSYS2 侧补装 mingw-w64 头文件与 winpthreads（rusqlite bundled 编译 SQLite 所需）；Rust 链接统一启用 `link-self-contained`。
- **Windows 应用清单改由 `build.rs` 统一注入**：关闭 tauri-build 的默认清单（`WindowsAttributes::new_without_app_manifest`），改由 `build.rs` 对所有链接目标追加同一份 `windows-app.manifest`。此前 tauri-build 只给 bin 目标嵌清单（内部走 `embed-resource` 的 `rustc-link-arg-bins`），`cargo test` 产出的可执行文件完全没有清单，导致 tao 静态导入的 `comctl32!TaskDialogIndirect`（仅 Common-Controls v6 导出）解析失败、测试在加载期以 `0xc0000139` 整体退出。现在 MSVC 走 `/MANIFEST:EMBED` + `/MANIFESTINPUT`，GNU 走 windres 资源对象，bin 与测试目标共用唯一一份清单来源。
- CI：测试步骤改为 `cargo test --lib` + `cargo test --test public_api`；仅编译不运行的那一步改用 `--message-format=json` 把 cargo 实际产出的测试可执行文件路径写盘，避免被 Cargo 缓存还原回来的历史产物干扰；原先的「导入表诊断」替换为**应用清单校验**（直接解析 exe 的 `RT_MANIFEST` 资源并断言含 Common-Controls v6），由 `continue-on-error` 的哨兵升级为硬性失败，故障信息也从「没有上下文的退出码」变成明确的原因。

### Fixed
- **非空目录递归删除在工作区内被误拒**：`native/tools.rs` 与 `fs_ops.rs` 都用未规范化的 `dirs.workspace` 去 `Path::strip_prefix` 一个已规范化的绝对路径，Windows 上因 `\\?\` 前缀 / 短名 / 大小写差异必然失配，导致工作区内的非空目录也无法递归删除。现统一改用新增的 `permissions::is_within`（两侧先规范化 + 大小写不敏感的前缀+分隔符匹配），并补了回归测试。
- **Worker 未配置时的报错不指向根因**：`plan` / `run_task` / `remember` / `recall` / `forget` 都先校验请求载荷、后检查配置，未配置时返回的是「缺少 goal」这类字段错误。现改为配置类前置条件优先，报错明确提示先 `configure`。
- **`agent_config` 单测相互踩踏**：测试目录按进程 id 复用，并行执行时写损坏 JSON 的用例会让读回用例拿到 `None` 而随机失败。改为每个用例独立临时目录。

## [1.0.0-alpha.1] - 2026-09-12

DeepHarness 重构启动：产品全面更名，确立「零配置、三 Agent 隔离」的新定位。

### Added
- **阶段 1 · 脚手架迁移**：更名 DeepHarness；React 19 + zustand 5 + Vite 6；TypeScript 全量 strict 模式；窗口启用 MicaDark 云母材质（网页背景透明）；全新品牌图标（挽具环母题，`scripts/gen_icons.py` 程序化生成）；启动动画改为 Logo 由模糊到清晰浮现（约 1.8 秒、不阻塞初始化）；配色收敛为钢蓝 → 青瓷单一渐变。
- **阶段 2 · Rust 权限层**：
  - 每 Agent 独立文件访问白名单（`permissions.json`，含读 / 读写模式、授权时间与来源，原子落盘）；
  - Agent 自有 `workspace/` 目录隐式可读写，外部路径必须经用户显式授权；
  - 规范化路径前缀匹配，杜绝 `C:\dir` 授权被 `C:\dir-evil` 绕过；容忍写尚不存在的新文件；
  - 安全文件命令集：`read_text_file` / `read_file_base64` / `write_text_file` / `list_directory` / `create_directory` / `delete_path`（非空目录仅限工作区内递归删除）；
  - 授权管理命令：`check_access` / `grant_access` / `revoke_access` / `list_grants` / `agent_dirs_info`。
- **阶段 3 · Agent 隔离层**：
  - `AgentRuntime` 抽象接口 + 注册表，三个 Agent 状态相互独立；
  - DeepSeek Harness 运行时：dsh 子进程管理、就绪探测、独立日志；
  - Codex 运行时：`CODEX_HOME` 隔离、`codex exec` 流式输出转发（日志 + 前端事件流）、按需拉起；
  - DeepHarness 自研 Agent：`--agent-worker` 自我重执行 Sidecar，stdin/stdout JSON Lines 协议（ping/echo/status/shutdown），请求-响应往返、优雅停止 + 超时强杀；
  - Windows Job Object：每 Agent 独立内存配额 + kill-on-close，主应用退出自动回收全部子进程；
  - 每 Agent 独立会话存储（`agents/<id>/sessions/`，原子写、单文件损坏自动跳过）与独立日志目录。

### Changed
- **移除无限制的 `run_command`**：旧版可执行任意系统命令的 IPC 命令已删除；Agent 文件工具仅剩白名单内的 `list_dir` / `read_file` / `write_file`，越权时向模型返回明确的授权引导话术。
- 仓库由 `DeepSeek-Harness-Desktop` 重命名为 **DeepHarness**，更新全部链接与更新检查 URL。
- Cargo 侧引入 `anyhow` / `thiserror` / `tracing`（双输出：stdout + 按日滚动文件）/ `uuid` / `chrono` / `dunce` / `base64` / `win32job`。

## [0.4.0] - 2026-08-30

### Added
- **内置官方 DeepSeek Harness**：新增 `npm run setup:resources` 一键准备脚本（`scripts/setup-resources.sh`），自动下载便携版 Node 并安装 `@deepseek-ai/dsh`；启动时由 Rust 后端 `start_dsh` 拉起官方 Web UI（`127.0.0.1:3080`），双击即用、插件生态与官方一致。
- **品牌启动动画**：深空星野 + 鲸鱼辉光 + 环形轨道 + 扫描进度，并实时展示「正在启动 DeepSeek Harness 服务…」及失败重试。
- **官方模型实时同步**（浏览器预览模式）：调用 `GET /v1/models` 拉取 DeepSeek 官方最新模型，启动自动同步 + 设置页手动刷新。
- **官方主页（GitHub Pages）**：新增 `docs/index.html`，含 Hero、特性、应用预览、安装、FAQ、技术栈；深空科幻配色与动效。
- **安装程序品牌化**：NSIS 接入 `headerImage` / `sidebarImage`，重绘安装图片（多层轨道环、双层辉光、扫描光带）。

### Changed
- **应用图标**：改用 DeepSeek 官方 Harness logo（`deepseek-ai/deepseek-harness` 的 `apps/web/public/favicon.svg`），重新生成全套图标。
- **仓库重命名**：`StarCore` → `DeepSeek-Harness-Desktop`，同步更新 README 徽章、下载链接与更新检查 URL。
- **产品定位**：由「自建聊天界面」调整为「内置官方 Harness 的桌面封装」，React 前端降级为浏览器预览模式。
- **Logo 组件**：由硬编码黑色改为品牌渐变（原黑色在深色主题下不可见）。
- **README**：更新定位说明、构建步骤（setup:resources）与商标归属。

### Fixed
- 修复 `bundle.resources` 引用不存在的 `resources/` 目录导致打包失败（现由 setup 脚本生成资源，并恢复正确的资源打包）。
- 修复 `selectModel` 误把模型 id 写入「选中会话」存储 `dh.selected`，导致刷新后回退到首个会话。
- 修复 Logo 在深色主题下不可见。
- 移除未经验证的 `deepseek-v4-flash-vision-exp` 内置模型。

## [0.3.0] - 历史版本

> 0.3.0 及更早版本未维护变更日志，此处仅作占位。

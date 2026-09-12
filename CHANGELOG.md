# Changelog

本项目的所有值得注意的变更都会记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)（SemVer 2.0.0）。

## [Unreleased]

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

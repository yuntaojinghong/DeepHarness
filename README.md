# DeepSeek Harness 桌面版

<p align="center">
  <a href="./README_EN.md">English</a> · <b>中文</b>
</p>

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="GitHub stars" src="https://img.shields.io/github/stars/yuntaojinghong/DeepHarness?style=social">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db.svg">
  <img alt="React" src="https://img.shields.io/badge/React-18-61dafb.svg">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178c6.svg">
</p>

把官方 **DeepSeek Harness**（`dsh`）打包进桌面的开箱即用客户端：双击即用，内置便携版 Node 运行时与官方 Harness，启动即加载官方 Web UI——无需手动装 Node、跑命令或配置环境。

> **星核 StarCore**：驾驭星辰智能的能量核心——科幻风 AI 工作台。

## ✨ 功能

- **内置官方 Harness**：自动打包官方 `@deepseek-ai/dsh`，双击启动官方 Web UI（`127.0.0.1:3080`），插件生态与官方完全一致
- **一键准备资源**：`npm run setup:resources` 自动下载便携 Node + 安装官方 Harness，无需手动操作
- **品牌启动动画**：深空星野 + 鲸鱼标志 + 环形轨道 + 扫描进度，服务就绪后自动进入
- **便携模式**：数据跟随应用目录（`portable.flag` 标记），整个文件夹拷到 U 盘即用
- **系统托盘**：关窗最小化到托盘，任务不中断；回合完成弹桌面通知
- **科幻主题**：深空霓虹配色、星空背景、发光动效，深浅双主题 + 字号/密度自定义

> 附带的 React 前端作为**浏览器预览模式**（`npm run dev`）提供：流式对话、多模型切换、官方模型实时同步、Agent 工具调用、人设卡、会话导出 MD / JSON、全文搜索、全本地存储、零遥测。

## 🛠 技术栈

- **桌面壳**：Tauri 2（Rust 后端，安装包 ~10MB + 内置运行时）
- **核心**：官方 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（`@deepseek-ai/dsh`，MIT）+ 便携版 Node 运行时
- **前端**：React 18 + TypeScript + Vite（浏览器预览模式）

## 🚀 开发运行

### 前置条件

1. **Node.js** ≥ 18（项目已用 Node 22）
2. **Rust**：`rustup` 安装 stable-msvc
3. **Visual Studio Build Tools**（含「使用 C++ 的桌面开发」工作负载）— Windows 上链接 MSVC 必需

```bash
# 安装依赖
npm install

# 一键准备内置资源（下载便携 Node + 安装官方 Harness，打包前必须执行）
npm run setup:resources

# 仅前端预览（浏览器，无系统能力）
npm run dev

# 完整桌面应用（开发模式，自动启动 Rust 后端）
npm run tauri dev

# 打包安装程序
npm run tauri build
```

> `npm run setup:resources` 会下载便携版 Node 并安装 `@deepseek-ai/dsh` 到 `resources/`（已 gitignore），产物随安装包内置，用户无需自行安装 Node。

## 📥 下载安装

前往 [Releases](https://github.com/yuntaojinghong/DeepHarness/releases) 下载最新的 `DeepHarness_*.exe` 安装包，双击安装即可（默认装到用户目录，无需管理员权限，自动创建桌面快捷方式）。

> 想绿色便携使用？在安装目录旁放置一个空的 `portable.flag` 文件，数据就会跟随应用目录存储，整个文件夹拷到 U 盘即可随处使用。

## 🎬 首次使用

1. 启动后按欢迎页引导，填入 DeepSeek API Key（platform.deepseek.com 获取）并选择工作目录
2. 顶部下拉选择模型（V3 / R1 等）
3. 直接开始对话；模型支持工具调用时，可让它帮你执行命令、读目录

## 📁 项目结构

```
deepseek-harness-desktop/
├── src/                    # React 前端
│   ├── App.tsx             # 布局、启动动画、主题
│   ├── store.ts            # zustand 全局状态（含官方模型实时拉取）
│   ├── components/         # 界面组件（含 SplashScreen 启动动画）
│   ├── lib/                # llm 流式客户端 / env 检测 / storage / models
│   └── styles/global.css   # 设计系统（含启动动画关键帧）
└── src-tauri/              # Rust 后端
    ├── src/lib.rs          # check_env / run_command / list_dir / 存储 / 托盘 / 通知 / update
    ├── tauri.conf.json
    └── Cargo.toml
```

## ⭐ 支持项目

如果这个项目对你有帮助，欢迎点个 **Star ⭐**，你的支持是持续更新的动力！

## 📄 License

[MIT](./LICENSE) © 2026 [yuntaojinghong](https://github.com/yuntaojinghong)

## 🤝 贡献

欢迎提交 Issue 与 Pull Request。提交前请确保前端 `npm run build` 与后端 `cargo check` 均通过。

## ™️ 商标

应用图标及品牌素材（鲸鱼 logo）来自 **DeepSeek** 官方开源仓库，仅用于标识本项目与 DeepSeek 的关系。本项目是独立开发的第三方桌面封装工具，与 DeepSeek 公司无官方隶属关系。如需将本项目用于商业再分发，请自行评估并遵守 DeepSeek 的品牌使用条款。

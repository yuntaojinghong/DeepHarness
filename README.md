# DeepHarness

<p align="center">
  <a href="./README_EN.md">English</a> · <b>中文</b>
</p>

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="GitHub stars" src="https://img.shields.io/github/stars/yuntaojinghong/DeepHarness?style=social">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db.svg">
  <img alt="React" src="https://img.shields.io/badge/React-19-61dafb.svg">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178c6.svg">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-dea584.svg">
</p>

**三个相互隔离的 Agent 工作台，装完即用。**

DeepHarness 是一个 Windows 桌面客户端：把官方 DeepSeek Harness（`dsh`）连同便携版 Node 运行时一起装进安装包，同时提供两个自家 Agent。三个 Agent 在进程、工作区、会话、配置、日志、插件与内存配额上**完全隔离**——一个崩溃、越权或跑飞，都不会影响另外两个。

## ✨ 三个 Agent

### 1. DeepSeek Harness

官方 `@deepseek-ai/dsh` 运行时，随包内置（含便携版 Node），无需自行安装。

dsh 0.1.5 起官方 Web UI 需要**每次启动重新生成**的一次性 token，直接访问固定地址只会得到 401。DeepHarness 从 dsh 运行日志中解析出本次有效的地址，并交给系统默认浏览器打开；token 始终留在后端，界面与日志里一律脱敏为 `token=***`。

### 2. Codex

按需拉起本机已安装的 Codex CLI，独立 `CODEX_HOME`，流式输出转发到界面。启动前会探测本机 CLI 可用性，不可用时明确告知而非静默失败。

### 3. DeepHarness（自研原生 Agent）

不依赖任何外部 CLI 的自研循环，也是本项目真正的主体：

- **多步规划**：目标 → 结构化计划（工具步骤 / 纯回答步骤），非法输出带错误反馈自动重试，步骤数有上限防失控
- **工具执行**：读文件 / 写文件 / 列目录 / 建目录 / 删路径，**每一次调用都过权限白名单**
- **反思与重试**：每步执行后由模型结构化评审（成功与否 / 摘要 / 是否重试 / 建议），失败自动重试一次；模型不可用时退化为确定性策略，保证循环一定有出口
- **长期记忆**：SQLite（rusqlite bundled，免系统依赖）存 fact / preference / event / reflection 四类记忆，支持标签、重要度与分词召回；任务事件自动沉淀
- **插件系统**：见下

## 🔌 插件系统（`.dph-plugin`）

一个插件就是一个 zip：根目录放 `manifest.json` 与 `index.js`。

```js
// index.js
module.exports = {
  tools: {
    async count_lines(args, ctx) {
      const files = await ctx.listDir(ctx.workspace);   // 经宿主白名单校验
      ctx.log(`workspace 下有 ${files.length} 项`);
      return { ok: true, count: files.length };
    },
  },
};
```

装进哪个 Agent 的 `plugins/` 目录，就只有那个 Agent 能用它——插件作用域和会话、配置一样按 Agent 隔离。

**三重隔离，缺一不可：**

| 层 | 机制 | 挡住什么 |
| --- | --- | --- |
| 进程 | 每次工具调用起一个 `node` 子进程，返回即退出，超时直接杀 | 插件崩溃 / 死循环只影响这一次调用，不拖垮 Agent |
| 文件系统 | `node --permission --allow-fs-read=<插件目录>` | 越界读与**任何**写都被 Node 内核拒绝（`ERR_ACCESS_DENIED`） |
| 白名单 | `ctx.readFile` / `writeFile` / `listDir` 经 JSON Lines 回传宿主 | 与内置工具走**同一道** `PermissionStore` 闸门，未授权路径拿到的仍是「需要用户授权」而不是文件内容 |

插件与内置工具同名时**内置优先**——插件不能顶掉内置工具。工具目录在一次任务开始时冻结，避免执行到一半插件被卸载后留下悬空步骤。

插件是可选增强：随包 Node 缺失、宿主落地失败、清单读不动，任一前置条件不满足都只是「没有插件工具」，内置工具与 Agent 启动完全不受影响。

## 🔒 权限模型

- 每个 Agent 有**独立**的白名单文件（`permissions.json`），互不可见
- Agent 自有的 `workspace/` 隐式可读写；**外部路径必须由用户显式授权**（读 / 读写两种模式，记录授权时间与来源，原子落盘）
- 路径按规范化后的前缀匹配，`C:\dir` 的授权无法被 `C:\dir-evil` 绕过
- 旧版那个可以执行任意系统命令的 `run_command` **已被移除**，Agent 的文件能力只剩白名单内的几个工具，越权时向模型返回明确的授权引导话术
- 每个 Agent 独占一个 Windows Job Object：独立内存配额 + kill-on-close，主应用退出自动回收全部子进程

## 🛠 技术栈

- **桌面壳**：Tauri 2（Rust）
- **前端**：React 19 + TypeScript（全量 strict）+ Vite 6 + zustand 5
- **核心**：官方 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（`@deepseek-ai/dsh`，MIT）+ 便携版 Node
- **存储**：SQLite（长期记忆，bundled 免系统依赖）+ 每 Agent 独立会话目录（原子写、单文件损坏自动跳过）

### 一些实现取向

- **HTTP 走 Windows 原生 WinHTTP**（系统 SChannel TLS），不为一次 API 调用引入 OpenSSL 编译负担
- **所有控制台子进程都设 `CREATE_NO_WINDOW`**：从 GUI 拉起 `netstat` / `taskkill` / `node` / `cmd /C start` 时不闪黑框、不抢焦点。这类问题无法在别处统一注入，`CREATE_NO_WINDOW` 是**创建**标志，漏掉一个调用点就仍会闪窗，因此全部收敛到一个 `process` 模块
- **渲染期异常一律显示可读卡片**（`ErrorBoundary`：错误名、信息、组件栈、「复制详情 / 重新加载」），不做静默黑屏
- **全本地、零遥测**：API Key 只留在本机 Agent 隔离目录内，界面只回显尾号

## 🚀 开发运行

### 前置条件（Windows）

1. **Node.js** ≥ 22
2. **Rust**：`rustup` 安装 stable-msvc
3. **Visual Studio Build Tools**（含「使用 C++ 的桌面开发」工作负载）——MSVC 链接必需

```bash
npm install

# 一键准备内置资源（下载便携 Node + 安装官方 dsh，打包前必须执行）
npm run setup:resources

# 完整桌面应用（开发模式，自动启动 Rust 后端）
npm run tauri dev

# 仅前端预览（浏览器，无系统能力，用于调界面）
npm run dev

# 校验
npm run typecheck && npm test

# 打包安装程序
npm run tauri build
```

`npm run setup:resources` 会把便携版 Node 与 `@deepseek-ai/dsh` 装到 `resources/`（已 gitignore），打包时随安装包内置，用户侧无需安装任何东西。脚本是**版本驱动**的：随包 Node 与 dsh 版本与目标不一致时会自动重装，不需要手工删目录。

> **本地不装 Rust 工具链也能改前端**：`npm run typecheck` / `npm test` / `npm run build` 都不需要 Rust。Rust 侧的改动请依赖 CI 验证。

## 📥 下载安装

前往 [Releases](https://github.com/yuntaojinghong/DeepHarness/releases) 下载最新的 `DeepHarness_*.exe`，双击安装即可（默认装到用户目录，无需管理员权限，自动创建桌面快捷方式）。

安装包约 **55MB**——因为内置了 `node.exe`（单文件约 70MB，压缩后占比最大）与整棵 dsh 依赖树。这是刻意的取舍：**零配置优先于体积预算**，干净机器装完就能直接用 Harness Agent，不必再单独下载运行时。

> 想绿色便携使用？在安装目录旁放一个空的 `portable.flag` 文件，数据就会跟随应用目录存储，整个文件夹拷到 U 盘即可随处使用。

## 🎬 首次使用

1. 启动后按欢迎页引导，决定是否填入 DeepSeek API Key（[platform.deepseek.com](https://platform.deepseek.com) 获取）
2. 左侧切换 Agent；中间是对话 / 任务视图，右侧是上下文与配置面板
3. 要用自研 Agent 的任务控制台：在右侧面板配置模型 → 输入目标 → 「生成计划」预览步骤 → 「执行任务」看逐步时间线（工具、结果、反思建议、重试与步骤修订标注）→ 任务总结
4. 要用插件：右侧面板「插件」区域选择 `.dph-plugin` 文件安装，装完即热重载，不必重启应用

## 📁 项目结构

```
DeepHarness/
├── src/                          # React 前端（唯一主界面）
│   ├── App.tsx                   # 布局、启动动画、错误边界
│   ├── store.ts                  # zustand 全局状态
│   ├── components/
│   │   ├── ChatArea.tsx          # 对话视图
│   │   ├── DeepHarnessView.tsx   # 任务控制台（计划 / 执行时间线 / 总结）
│   │   ├── DeepHarnessPanel.tsx  # 右侧面板（模型配置 / 记忆 / 插件）
│   │   └── PluginSection.tsx     # 插件安装、启停、卸载
│   ├── lib/
│   │   ├── deepharness.ts        # Agent 契约层（与 Rust 命令一一对应）
│   │   ├── plugins.ts            # 插件契约层
│   │   ├── hooks.ts              # store 选择器收敛（只取状态切片）
│   │   └── selector-guard.test.ts# 静态守卫：禁止把会新建引用的方法塞进选择器
│   └── styles/global.css         # 设计系统
├── src-tauri/
│   ├── plugin-host/host.cjs      # 插件宿主 shim（include_str! 内嵌进二进制）
│   └── src/
│       ├── permissions.rs        # 每 Agent 独立白名单 + 规范化路径匹配
│       ├── plugins/              # 插件：store（zip 解包）/ host（沙箱桥）/ commands
│       ├── native/               # 自研 Agent：planner / executor / reflector / tools / memory / model
│       ├── agents/               # 三 Agent 运行时与注册表
│       ├── open_url.rs           # 只允许回环地址与白名单公网的浏览器打开
│       └── process.rs            # 统一的静默子进程创建
├── scripts/
│   ├── setup-resources.sh        # 下载便携 Node + 安装 dsh（版本驱动）
│   ├── scan-peers.cjs            # 同行依赖扫描补齐
│   └── gen_icons.py              # 品牌图标程序化生成
└── docs/index.html               # GitHub Pages 主页
```

## 🧪 测试

```bash
npm test                                        # 前端单元测试（vitest）
cargo test --manifest-path src-tauri/Cargo.toml # Rust 单元测试 + 跨模块集成测试
```

Rust 侧除了各模块单测，还有一层**跨模块集成测试**（`src-tauri/tests/public_api.rs`）：只经公开接口访问，校验规划器 ↔ 工具注册表、serde 的 camelCase 契约、配置持久化与三 Agent 目录隔离等**跨模块约定**——这类约定最容易在重构中悄悄破掉。

CI 分三个 job 并行：前端（类型检查 / 单测 / 构建）、Rust（编译 + 测试 + 应用清单校验）、打包（NSIS 安装程序，产物缺失即判失败）。Rust job 会额外解析 exe 的 `RT_MANIFEST` 资源并断言含 Common-Controls v6——缺了它，tao 静态导入的 `comctl32!TaskDialogIndirect` 会让测试进程在加载期以 `0xc0000139` 整体退出。

## ⭐ 支持项目

如果这个项目对你有帮助，欢迎点个 **Star ⭐**。应用内标题栏也有「支持作者」入口。

## 🤝 贡献

欢迎提交 Issue 与 Pull Request。提交前请确保 `npm run typecheck`、`npm test`、`npm run build` 通过。

## 📄 License

[MIT](./LICENSE) © 2026 [yuntaojinghong](https://github.com/yuntaojinghong)

## ™️ 商标

本项目内置官方 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（MIT）作为其中一个 Agent 运行时，品牌文案中出现的「DeepSeek」指代该官方项目。DeepHarness 是独立开发的第三方桌面客户端，与 DeepSeek 公司无官方隶属关系。如需将本项目用于商业再分发，请自行评估并遵守相关品牌使用条款。

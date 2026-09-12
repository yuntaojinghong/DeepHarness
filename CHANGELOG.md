# Changelog

本项目的所有值得注意的变更都会记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)（SemVer 2.0.0）。

## [Unreleased]

### Added
- **「一键打开官方 Web UI」改为后端打开**（`agent_open_web_ui` + 新增模块 `open_url.rs`）：dsh 0.1.5 起官方 Web UI 需要**每次启动重新生成**的一次性 token（不带 token 访问直接 401，带旧 token 同样 401），且该 token 要等进程起来约 5 秒后才打印出来。前端原先固定 `window.open("http://127.0.0.1:3080")`，在新版本上只会打开一个 401 页面；而「await 拿到地址再 open」又会丢失用户手势（可能被弹窗拦截、或落到一个拿不到句柄的新窗口上）。现由 Rust 侧从 dsh 运行日志中解析出本次有效的地址并在系统默认浏览器中打开，同时把 token 留在后端、错误信息与返回文案一律脱敏（`token=***`）。新增的 `open_url` 模块只接受回环地址且字符集白名单化的 URL（拒绝 `& | ^ " ' \`` 空格与反斜杠），避免 `cmd /C start` 重新解析命令行时留下 shell 注入面，并配套 11 个单元测试。
- **同行依赖扫描器**（`scripts/lib/scan-peers.cjs`）：遍历已安装包的 `package.json`，按 Node 的向上查找规则与粗粒度 semver 判断列出未被满足的 `peerDependencies`（跳过 `optional`），输出 `名字@范围` 规格交给 npm 显式安装。
- 前端契约层新增 `agentOpenWebUi()`（`src/lib/deepharness.ts`），并补了「浏览器预览下必须明确拒绝而非静默打开 401 页面」的测试。
- **「支持作者」入口**（标题栏爱心按钮 → `SupportModal`）：一个弹窗收纳两种支持方式 ——「去点 Star」（跳转仓库主页）与「请作者喝杯咖啡」（微信赞赏码，直接可扫）。赞赏码由原图裁出纯二维码面板后按最近邻放大（`src/assets/support-qrcode.png`，860×860），避免把原图里的浅色边框与色条带进深色主题；图片底色固定为白（不能用主题变量，否则会压掉码的对比度导致识别失败）。链接与作者署名集中在 `src/lib/links.ts`，含 9 个单元测试（其中一项与 Rust 侧白名单做**跨层契约校验**，防止两端各自放宽后悄悄偏离）。
- **`open_url` 模块扩展公网白名单**：新增 `open_external_url` 命令，允许 `https://github.com` 与 `https://platform.deepseek.com` 及其子域（仅 https、host 精确匹配或 `.` 后缀，`https://github.com.evil.com` 与 `https://evilgithub.com` 均被拒），并补 6 个测试。前端原先用 `<a target="_blank">` / `window.open` 打开 GitHub 与更新下载页，在 Tauri WebView 下可能被 WebView 自己接管（替换应用界面）或直接无效；现统一走后端交给系统浏览器。
- **Windows 安装包打包流水线**：CI 新增 `package` job，在 `windows-latest` 上用 `cargo tauri build` 产出 NSIS 安装程序并上传为产物，产物缺失时步骤直接失败（而非告警）——「CI 全绿却没有安装包」是最需要防住的结果。该 job 与前端、Rust 两个 job **并行**：release 编译复用了不了 debug 的 target 目录，串行只会把两者的耗时叠加到关键路径上。缓存单独用 `cargo-release-*` 键，因为 `target/release` 与 `target/debug` 的产物不通用，共用一个键会让两边都命中不了。
- **安装包内嵌随包资源**（`bundle.resources` → `../resources/**/*`）：Node 便携版与 dsh 依赖树由 `setup-resources.sh` 在打包前生成并打进安装程序，干净机器装完即可用 Harness Agent，不必再单独下载。代价是体积：`node.exe` 单文件约 70MB，安装包最终 **55MB**，超出原定 ≤30MB 的预算——这是明确取舍后的结果（零配置优先于体积预算）。
- **`resources/README.md` 占位文件**：`tauri-build` 在**编译期**就会展开 `bundle.resources` 的 glob 并校验，目录完全不存在会让 `cargo check` / `cargo test` / `cargo tauri build` 全部失败。占位文件保证 glob 至少匹配一项，使**没有资源的环境也能正常编译**（新克隆的仓库、只跑测试的 Rust job），并就地记录了这个约束。
- **release 体积优化**（`[profile.release]`）：`opt-level = "z"` + `lto = "thin"` + `strip = true`。默认 release 配置会保留符号表与调试信息，二进制远大于必要体积；thin LTO 的编译耗时接近默认，仍能裁掉未被引用的代码。
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
- **阶段 4 · 前端三 Agent 主界面与 DeepHarness 任务控制台**：
  - 自研 React 界面成为唯一主界面：左侧 Agent 切换 + 会话列表，中间对话 / 任务视图，右侧上下文面板；官方 Web UI 降级为标题栏的「在浏览器中打开」可选按钮（仅在 Harness 运行时可用）。
  - 新增 Agent 契约层 `src/lib/deepharness.ts`：统一封装 `agent_list` / `agent_start` / `agent_stop` / `agent_status` 与 9 个 `deepharness_*` 命令，含 Agent 元信息、状态文案与浏览器预览降级，所有类型与 Rust 侧 camelCase 序列化一一对应。
  - 会话按 Agent 隔离：`Conversation` 新增 `agent` 字段（历史数据读取时统一兜底），切换 Agent 时会话列表、主视图与右侧面板整体切换，不跨 Agent 泄漏内容。
  - DeepHarness 任务控制台：目标输入 → 「生成计划」预览结构化步骤 → 「执行任务」展示逐步执行时间线（工具、结果、反思建议、重试与步骤修订标注）→ 任务总结；未启动 Worker 或未配置模型时给出明确引导而非静默失败。
  - DeepHarness 右侧面板：Worker 就绪状态（进程号、协议版本、崩溃原因）与启停、模型配置表单（落盘 + 热注入，API Key 只回显尾号，支持一键沿用对话侧已填密钥）、长期记忆库（统计 / 关键词检索 / 手动写入 / 单条删除）。
  - 前端单元测试：新增 `src/lib/deepharness.test.ts`、`src/lib/storage.test.ts` 与 `src/lib/links.test.ts`（45 个用例，覆盖状态映射、错误归一化、截断与序列化辅助、会话字段兜底、模型去重、浏览器预览降级与「Web UI 必须由后端打开」的守卫、对外链接与后端白名单的跨层一致性，以及存储不可用 / 数据损坏时的降级路径）。

### Changed
- **随包资源版本升级**：Node 便携版 22.12.0 → **22.22.2**，`@deepseek-ai/dsh` → **0.1.5-rc.2**。dsh 的 code-runtime 使用 `node:module` 的 `stripTypeScriptTypes()`（Node 22.13 引入），22.12.0 下整个 Harness 会在加载期报 `does not provide an export named 'stripTypeScriptTypes'`；资源准备脚本现按 major.minor 校验随包 Node，版本不足会重新下载，不再要求手工删目录。
- **资源准备脚本改为版本驱动**：「已就绪」判断新增版本维度，已装 dsh 与目标 `DSH_VERSION` 不一致时自动重装，升级路径不再被「已就绪」永久挡住。
- **dsh 依赖安装改用 `--legacy-peer-deps`**：dsh 有 178 个互相声明的 `@deepseek-ai/*` 包，npm 的同行依赖解析在这张图上会卡死在 `placeDep` 阶段（实测 12 分钟无任何进展）。跳过自动同行解析后完整安装只需十几秒，代价是同行依赖不自动装，因此紧接着由 `scan-peers.cjs` 扫描缺失项并显式补齐（实测：扫描 520 个包、0 缺失）。
- `AgentRuntime` trait 新增默认方法 `web_ui_url()`：约定实现必须返回**带鉴权参数、可直接打开**的地址，未就绪时返回 `None`。等待逻辑放在阻塞线程池上执行且不持有注册表锁，避免拖住另外两个 Agent 的状态查询。
- **DeepHarness Worker 就绪状态改为单一来源**：`workerReadiness` 与 `refreshWorkerReadiness` 提升到全局 store，由 `App` 统一轮询，任务控制台与右侧面板只读结果。此前两处各自 `setInterval` 轮询同一个 `deepharness_status`，既重复发 IPC，又可能短暂呈现互相矛盾的结论。
- **启动流程不再跳转官方 Web UI**：移除启动时 `window.location.href = "http://127.0.0.1:3080"` 与已不存在的 `start_dsh` 命令调用（该命令早已在重构中删除，此前会使每次启动都落到「启动失败」分支）。现在启动只做本机环境检测与 Agent 概览读取，失败也不阻塞进入界面，保证零配置用户直接可用。
- **前端 CI 增加单元测试步骤**（`npm test`），与类型检查、构建并列。
- Windows 构建链补齐：MSYS2 侧补装 mingw-w64 头文件与 winpthreads（rusqlite bundled 编译 SQLite 所需）；Rust 链接统一启用 `link-self-contained`。
- **Windows 应用清单改由 `build.rs` 统一注入**：关闭 tauri-build 的默认清单（`WindowsAttributes::new_without_app_manifest`），改由 `build.rs` 对所有链接目标追加同一份 `windows-app.manifest`。此前 tauri-build 只给 bin 目标嵌清单（内部走 `embed-resource` 的 `rustc-link-arg-bins`），`cargo test` 产出的可执行文件完全没有清单，导致 tao 静态导入的 `comctl32!TaskDialogIndirect`（仅 Common-Controls v6 导出）解析失败、测试在加载期以 `0xc0000139` 整体退出。现在 MSVC 走 `/MANIFEST:EMBED` + `/MANIFESTINPUT`，GNU 走 windres 资源对象，bin 与测试目标共用唯一一份清单来源。
- CI：测试步骤改为 `cargo test --lib` + `cargo test --test public_api`；仅编译不运行的那一步改用 `--message-format=json` 把 cargo 实际产出的测试可执行文件路径写盘，避免被 Cargo 缓存还原回来的历史产物干扰；原先的「导入表诊断」替换为**应用清单校验**（直接解析 exe 的 `RT_MANIFEST` 资源并断言含 Common-Controls v6），由 `continue-on-error` 的哨兵升级为硬性失败，故障信息也从「没有上下文的退出码」变成明确的原因。

### Fixed
- **启动动画结束后整屏黑屏（界面其实已崩溃）**：`Sidebar` 把 `searchConversations()` 直接当成 zustand 选择器（`useAppStore((s) => s.searchConversations())`）。该方法的实现是「按当前 Agent 过滤会话并叠加搜索词」，一路走到 `Array.prototype.filter`，**每次调用都返回新数组**；zustand v5 用 `Object.is` 比较选择器返回值，恒不相等 → React 判定快照持续变化 → 无限重渲染 → 触发 `Minified React error #185`（Maximum update depth exceeded）→ React 卸载整棵组件树，`#root` 变空，只剩 `body` 的深色底。生产构建会把 `getSnapshot should be cached` 这类开发期告警一并剥掉，所以控制台与日志全空，看起来就像「启动动画播完就黑屏」。**这也解释了此前「从来没看见过三个 Agent」**——上一轮窗口透明（已修）与本次崩溃，两种原因都会表现为「什么都看不见」。修复方式：新增 `src/lib/hooks.ts` 把选择器收敛为「只取状态切片」，派生计算一律交给 `useMemo`（`useActiveConversation` / `useVisibleConversations` / `useAgentStatus`），6 个组件改走这些钩子；派生逻辑抽到纯函数模块 `src/lib/conversations.ts`，`store.ts` 复用它并在方法前标注「不可作为选择器返回值」。同时新增 `ErrorBoundary` 挂在 `App` 外层：此后任何**渲染期**异常都会显示含错误名、信息、组件栈与「复制详情 / 重新加载」按钮的可读卡片，而不是静默黑屏。
- **防止「选择器返回值不稳定」缺陷复发**：新增 `src/lib/selector-guard.test.ts`，静态扫描全部源码，禁止 `useAppStore((s) => s.某方法(` 这种把会新建引用的 store 方法塞进选择器的写法（参数名与接收者用反向引用绑定为同一标识符，避免误判 `s.x.some(...)` 这类返回布尔值的安全写法）；并附覆盖度断言，确保扫描到的文件数与 `store.ts` / `hooks.ts` 的存在性，避免守卫静默空转。
- **欢迎弹窗的品牌文案残留**：`WelcomeModal` 标题仍是「欢迎使用星核 StarCore」，改为「欢迎使用 DeepHarness」。
- **窗口全透明、界面像没渲染出来**：窗口原先同时启用了 `transparent: true` 与 `windowEffects.micaDark`，并靠 `html.mica body { background: transparent }` 让云母材质透出。但 **Mica 只有 Windows 11 支持**，在 Windows 10（实测 build 19045）上材质不生效，而窗口与网页背景**同时**是透明的，于是整个窗口只剩一层毛玻璃——能直接看见底下的窗口，三个 Agent 与全部界面都不可见。现已移除窗口透明与 Mica 效果，背景统一由 `body` 的 `var(--bg)` 提供，Win10 / Win11 上都表现为正常的不透明窗口。
- **启动与使用过程中会莫名弹出命令行黑框**：所有控制台子进程（`netstat` / `taskkill`、`where`、`codex`、`curl`、`node` 启动 dsh、`cmd /C start` 打开浏览器）都未设置创建标志，从 GUI 进程拉起时各自获得一个新建的控制台并闪现在屏幕上，还会抢焦点。现新增 `process` 模块收敛这一处理（`CREATE_NO_WINDOW` + `CREATE_NEW_PROCESS_GROUP`），并让**全部**创建子进程的调用点经由 `process::command`；配套 3 个单元测试，其中一个真实执行 `cmd` 验证标志未被忽略。这类问题无法在别处统一注入——`CREATE_NO_WINDOW` 是创建标志，漏掉任何一个调用点就仍会闪窗。
- **安装程序显示的是上一个项目的品牌**：`installer-header.bmp` / `installer-sidebar.bmp` 是旧项目（StarCore）的素材——鲸鱼图形、「DeepSeek Harness」字样、「v0.3.0」与「STARCORE」落款，与本项目毫无关系。现新增 `scripts/make-installer-art.py`，按 `Logo.tsx` 的真实品牌几何（石墨深空底 + 挽具环 + 核心 + 三节点）重绘两张素材；**刻意不再在图内写版本号**，旧素材的「v0.3.0」正是硬编码版本留下的债，不重复这个错误。
- **官方 Web UI 在新版 dsh 上必然 401**：dsh 0.1.5 起 Web UI 需要每次启动重新生成的 token，原先固定打开基础地址只能得到 401 页面。现由后端解析带 token 的地址并打开（见 Added 第一条）。
- **资源准备脚本的「Node 版本不足时替换失败」**：旧实现把解压结果 `mv` 到一个可能已存在的 `resources/node` 下，结果是 `resources/node/node-v22.22.2-win-x64/` —— 看着像成功，但 bin 位置全错、版本依旧过旧。现改为先校验新 Node 可执行、再把旧目录改名暂存并替换，失败回滚，保证不会只剩一个不可用的目录。
- **资源准备脚本的「完好依赖树被判定为残缺」**：就绪判断原先递归 `find` 所有子目录，把包内部的 `dist/`、`lib/` 这类正常子目录当成「缺少 `package.json` 的残包」，于是每次都触发整棵重装（非幂等、每次白等 40 秒）。现只匹配真正的包目录形态（`node_modules/<name>` 与 `node_modules/@scope/<name>`）。
- **资源准备脚本的「残缺依赖树被永久跳过」**：旧判断只看 `bin.js` 是否存在，一次被中断的 `npm install` 留下的坏树（实测 163 个包缺 `package.json`、60 个空目录、无 `package-lock.json`）会被判定为就绪而再不自愈，表现为 dsh 启动即 `Cannot find package 'js-yaml'`。现同时校验安装清单、包目录完整性与同行依赖完整性，任一不满足即整棵重建（就地补装会与删除策略打架、越修越坏）。
- **资源准备脚本在 Git Bash 下的路径问题**：Windows 自带的 `curl` / PowerShell 不认 MSYS 风格路径（`/c/Users/...`），下载会以 `Failed to open the file ...: No such file or directory` 失败；把同一路径当参数传给原生 `node.exe` 会被解析成 `C:\c\Users\...`，导致 npm 直接 `MODULE_NOT_FOUND`。现下载/解压一律在临时目录内用相对路径进行，并把交给原生 exe 的路径经 `cygpath` 转换。
- **同行依赖扫描器的诊断信息污染安装参数**：调用方误用 `2>&1` 合并 stderr 后，把 `[scan-peers] 扫描 520 个包，缺失同行依赖 0 个` 当成包规格传给 npm，报 `EINVALIDTAGNAME`。现只取 stdout 作为结果、stderr 落到独立文件仅用于报错，并把脚本内告警统一改为写 stderr（避免污染任何 `$(...)` 取值）。
- **npm 调用不再依赖宿主机装过 Node**：统一经随包 Node 自带的 npm（`node.exe <npm-cli.js>`）调用，既保证 npm 与 node 版本匹配，也避免 Windows 上 `.cmd` 经 shell 转发带来的引号与编码问题。
- **本机存储不可用时界面无法启动**：`storage.ts` 原先直接访问 `localStorage`，WebView 在隐私模式 / 存储被禁用时访问会**抛异常**（而非返回 `null`），异常会一路冒到 store 的初始化解，导致整个界面白屏。现统一经 `readRaw` / `writeRaw` / `readJson` 容错助手访问，读失败当无数据、写失败静默跳过，并补了对应的降级测试。
- **非空目录递归删除在工作区内被误拒**：`native/tools.rs` 与 `fs_ops.rs` 都用未规范化的 `dirs.workspace` 去 `Path::strip_prefix` 一个已规范化的绝对路径，Windows 上因 `\\?\` 前缀 / 短名 / 大小写差异必然失配，导致工作区内的非空目录也无法递归删除。现统一改用新增的 `permissions::is_within`（两侧先规范化 + 大小写不敏感的前缀+分隔符匹配），并补了回归测试。
- **Worker 未配置时的报错不指向根因**：`plan` / `run_task` / `remember` / `recall` / `forget` 都先校验请求载荷、后检查配置，未配置时返回的是「缺少 goal」这类字段错误。现改为配置类前置条件优先，报错明确提示先 `configure`。
- **`agent_config` 单测相互踩踏**：测试目录按进程 id 复用，并行执行时写损坏 JSON 的用例会让读回用例拿到 `None` 而随机失败。改为每个用例独立临时目录。
- **CI 的 Windows Python 步骤因中文输出失败**：runner 上 Python 的 stdout 默认按 cp1252（charmap）编码，`shell: python` 步骤里任何中文 print 都会抛 `UnicodeEncodeError` 并把步骤判为失败——应用清单校验因此在「已经校验完第一个二进制、正要打印成功提示」时中断。现于 job 级设置 `PYTHONUTF8` / `PYTHONIOENCODING`，并在两个内联脚本里显式 `reconfigure` stdout/stderr，使步骤不再依赖 runner 语言环境。

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

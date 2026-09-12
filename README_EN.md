# DeepHarness

<p align="center">
  <a href="./README.md">中文</a> · <b>English</b>
</p>

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="GitHub stars" src="https://img.shields.io/github/stars/yuntaojinghong/DeepHarness?style=social">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db.svg">
  <img alt="React" src="https://img.shields.io/badge/React-19-61dafb.svg">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178c6.svg">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-dea584.svg">
</p>

**Three mutually isolated agent workspaces, ready the moment you install.**

DeepHarness is a Windows desktop client that bundles the official DeepSeek Harness (`dsh`) together with a portable Node runtime, plus two agents of its own. All three are **fully isolated** from each other — process, workspace, sessions, config, logs, plugins and memory quota. One crashing, overstepping its permissions or running away never affects the other two.

## ✨ The three agents

### 1. DeepSeek Harness

The official `@deepseek-ai/dsh` runtime, shipped in the installer (portable Node included) — nothing to install yourself.

Since dsh 0.1.5 the official Web UI requires a **freshly generated one-time token on every launch**; hitting the fixed address just returns 401. DeepHarness parses the currently valid URL out of the dsh runtime log and hands it to your default browser. The token always stays in the backend — UI text and logs are redacted to `token=***`.

### 2. Codex

Launches your locally installed Codex CLI on demand, under an isolated `CODEX_HOME`, streaming its output into the UI. CLI availability is probed first, and an unavailable CLI is reported clearly rather than failing silently.

### 3. DeepHarness (native agent)

A self-contained loop with no external CLI dependency — and the real center of this project:

- **Multi-step planning**: goal → structured plan (tool steps / answer-only steps). Invalid output is retried with the error fed back; step count is capped so a confused model can't run away
- **Tool execution**: read / write file, list directory, create directory, delete path — **every single call goes through the permission whitelist**
- **Reflection & retry**: after each step the model produces a structured review (success / summary / shouldRetry / advice), and failures are retried once. When the model is unavailable this degrades to a deterministic policy so the loop always has an exit
- **Long-term memory**: SQLite (rusqlite bundled — no system dependency) holding fact / preference / event / reflection entries with tags, importance and tokenized recall; task events are distilled into memory automatically
- **Plugin system**: below

## 🔌 Plugins (`.dph-plugin`)

A plugin is a zip: `manifest.json` plus `index.js` at the root.

```js
// index.js
module.exports = {
  tools: {
    async count_lines(args, ctx) {
      const files = await ctx.listDir(ctx.workspace);   // validated by the host
      ctx.log(`workspace has ${files.length} entries`);
      return { ok: true, count: files.length };
    },
  },
};
```

Install it into one agent's `plugins/` directory and only that agent can use it — plugin scope is isolated per agent, exactly like sessions and config.

**Three layers of isolation, all required:**

| Layer | Mechanism | What it stops |
| --- | --- | --- |
| Process | A fresh `node` child process per tool call; exits on return, killed on timeout | A crashing or looping plugin affects only that one call |
| Filesystem | `node --permission --allow-fs-read=<plugin dir>` | Out-of-tree reads and **all** writes are rejected by the Node core (`ERR_ACCESS_DENIED`) |
| Whitelist | `ctx.readFile` / `writeFile` / `listDir` proxy back over JSON Lines | Same `PermissionStore` gate as builtin tools — an unauthorized path returns "needs authorization", not file contents |

When a plugin tool shares a name with a builtin one, **the builtin wins** — plugins cannot shadow builtins. The tool catalog is frozen when a task starts, so uninstalling a plugin mid-task can't leave a dangling step.

Plugins are strictly optional: a missing bundled Node, a failed host materialization, an unreadable manifest — any of these just means "no plugin tools". Builtin tools and agent startup are unaffected.

## 🔒 Permission model

- Every agent has its **own** whitelist file (`permissions.json`), invisible to the others
- An agent's own `workspace/` is implicitly readable and writable; **external paths must be explicitly authorized by you** (read or read-write, with grant time and origin recorded, written atomically)
- Matching is done on canonicalized path prefixes, so a grant for `C:\dir` cannot be bypassed by `C:\dir-evil`
- The old `run_command` that could execute arbitrary system commands has been **removed**. An agent's file capabilities are limited to the whitelisted tools above, and an out-of-scope attempt returns a clear "please authorize" message to the model
- Each agent gets its own Windows Job Object: separate memory quota plus kill-on-close, so quitting the app reclaims every child process

## 🛠 Tech stack

- **Shell**: Tauri 2 (Rust)
- **Frontend**: React 19 + TypeScript (fully strict) + Vite 6 + zustand 5
- **Core**: official [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (`@deepseek-ai/dsh`, MIT) + portable Node
- **Storage**: SQLite (long-term memory, bundled) + a per-agent session directory (atomic writes, corrupt files skipped individually)

### Implementation notes

- **HTTP goes through native WinHTTP** (system SChannel TLS) — no OpenSSL build cost for a few API calls
- **Every console child process sets `CREATE_NO_WINDOW`**: spawning `netstat` / `taskkill` / `node` / `cmd /C start` from a GUI process otherwise flashes a black window and steals focus. You cannot inject this centrally — it's a *creation* flag, so missing a single call site still flashes a window. All creation therefore funnels through one `process` module
- **Render-time exceptions always surface a readable card** (`ErrorBoundary`: error name, message, component stack, "copy details / reload") instead of a silent black screen
- **Fully local, zero telemetry**: your API key stays in the agent's isolated directory, and the UI only ever echoes its last few characters

## 🚀 Development

### Prerequisites (Windows)

1. **Node.js** ≥ 22
2. **Rust** via `rustup` (stable-msvc)
3. **Visual Studio Build Tools** with the "Desktop development with C++" workload (required for MSVC linking)

```bash
npm install

# One-shot: fetch portable Node + install the official dsh (required before packaging)
npm run setup:resources

# Full desktop app (dev mode, launches the Rust backend)
npm run tauri dev

# Frontend-only preview (browser, no system capabilities)
npm run dev

# Checks
npm run typecheck && npm test

# Build the installer
npm run tauri build
```

`npm run setup:resources` installs portable Node and `@deepseek-ai/dsh` into `resources/` (gitignored); the packaging step bundles them so users install nothing. The script is **version-driven**: when the bundled Node or dsh version doesn't match the target it reinstalls automatically — no manual directory deletion.

> **You can work on the frontend without a Rust toolchain**: `npm run typecheck`, `npm test` and `npm run build` need no Rust. Let CI verify Rust changes.

## 📥 Download & install

Grab the latest `DeepHarness_*.exe` from [Releases](https://github.com/yuntaojinghong/DeepHarness/releases) and double-click (installs to your user directory, no admin required, desktop shortcut created automatically).

The installer is ~**55MB** because it bundles `node.exe` (a single ~70MB binary, the bulk of the compressed size) plus the whole dsh dependency tree. That's a deliberate trade-off: **zero-config beats a size budget** — a clean machine can use the Harness agent immediately, with nothing extra to download.

> Prefer portable? Drop an empty `portable.flag` file next to the install directory and data will follow the app — copy the folder to a USB stick and use it anywhere.

## 🎬 First run

1. Follow the welcome wizard and decide whether to enter a DeepSeek API key (from [platform.deepseek.com](https://platform.deepseek.com))
2. Switch agents on the left; the middle is the conversation / task view, the right is context and configuration
3. To use the native agent's task console: configure the model in the right panel → enter a goal → **Generate plan** to preview the steps → **Run task** to watch the step timeline (tool, result, reflection advice, retries and step revisions) → task summary
4. To use plugins: pick a `.dph-plugin` file in the right panel's plugin section. It hot-reloads immediately — no restart

## 📁 Project structure

```
DeepHarness/
├── src/                          # React frontend (the one and only main UI)
│   ├── App.tsx                   # layout, splash animation, error boundary
│   ├── store.ts                  # zustand global state
│   ├── components/
│   │   ├── ChatArea.tsx          # conversation view
│   │   ├── DeepHarnessView.tsx   # task console (plan / timeline / summary)
│   │   ├── DeepHarnessPanel.tsx  # right panel (model config / memory / plugins)
│   │   └── PluginSection.tsx     # install, enable, uninstall plugins
│   ├── lib/
│   │   ├── deepharness.ts        # agent contract layer (mirrors the Rust commands)
│   │   ├── plugins.ts            # plugin contract layer
│   │   ├── hooks.ts              # store selectors narrowed to state slices
│   │   └── selector-guard.test.ts# static guard against unstable selectors
│   └── styles/global.css         # design system
├── src-tauri/
│   ├── plugin-host/host.cjs      # plugin host shim (embedded via include_str!)
│   └── src/
│       ├── permissions.rs        # per-agent whitelist + canonical path matching
│       ├── plugins/              # store (zip extraction) / host (sandbox bridge) / commands
│       ├── native/               # native agent: planner / executor / reflector / tools / memory / model
│       ├── agents/               # the three runtimes and their registry
│       ├── open_url.rs           # browser opening limited to loopback + a public whitelist
│       └── process.rs            # one place that creates silent child processes
├── scripts/
│   ├── setup-resources.sh        # fetch portable Node + install dsh (version-driven)
│   ├── scan-peers.cjs            # fill in missing peer dependencies
│   └── gen_icons.py              # programmatic brand icon generation
└── docs/index.html               # GitHub Pages site
```

## 🧪 Testing

```bash
npm test                                        # frontend unit tests (vitest)
cargo test --manifest-path src-tauri/Cargo.toml # Rust unit + cross-module integration tests
```

Beyond per-module unit tests, Rust has a **cross-module integration layer** (`src-tauri/tests/public_api.rs`) that touches only public interfaces and verifies the contracts most likely to rot silently during refactors: planner ↔ tool registry, the serde camelCase contract, config persistence, and per-agent directory isolation.

CI runs three parallel jobs: frontend (typecheck / unit tests / build), Rust (build + tests + app-manifest check), and packaging (NSIS installer, failing the step if the artifact is missing). The Rust job additionally parses the `RT_MANIFEST` resource out of the exe and asserts it contains Common-Controls v6 — without it, tao's static import of `comctl32!TaskDialogIndirect` makes the test process exit wholesale at load time with `0xc0000139`.

## ⭐ Support

If this project is useful to you, a **Star ⭐** is appreciated. There's also a "Support the author" entry in the title bar.

## 🤝 Contributing

Issues and pull requests are welcome. Please make sure `npm run typecheck`, `npm test` and `npm run build` pass before submitting.

## 📄 License

[MIT](./LICENSE) © 2026 [yuntaojinghong](https://github.com/yuntaojinghong)

## ™️ Trademarks

This project bundles the official [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (MIT) as one of its agent runtimes, and "DeepSeek" in the copy refers to that official project. DeepHarness is an independently developed third-party desktop client with no official affiliation to DeepSeek. If you plan to redistribute it commercially, please evaluate and comply with the relevant brand usage terms yourself.

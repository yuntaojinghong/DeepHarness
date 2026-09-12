# DeepSeek Harness Desktop

<p align="center">
  <a href="./README.md">中文</a> · <b>English</b>
</p>

<p align="center">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="GitHub stars" src="https://img.shields.io/github/stars/yuntaojinghong/DeepHarness?style=social">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db.svg">
  <img alt="React" src="https://img.shields.io/badge/React-18-61dafb.svg">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5-3178c6.svg">
</p>

A zero-config desktop client that bundles the official DeepSeek Harness — three mutually isolated agent workspaces, out of the box.

> **DeepHarness**: the desktop home for the official DeepSeek Harness runtime.

## ✨ Features

- **Streaming chat** with Markdown / code highlighting / tables / blockquotes
- **Multi-model switching**: live-sync the DeepSeek V4 Flash / Pro lineup (and any new official models) from the official API on launch — or hit "Refresh official models" in Settings; also supports any OpenAI-compatible service (Qwen / Kimi / GLM …), each with its own icon
- **Branded splash animation**: deep-space starfield, whale emblem, orbiting ring and scanning bar that fade into the main UI in 1.3s
- **Portable mode**: data follows the app directory (`portable.flag` marker) — copy the whole folder to a USB stick and run anywhere
- **System tray**: closing the window minimizes to tray without interrupting tasks; desktop notifications on turn completion
- **Environment self-check**: detects Python / Node / Git and auto-provisions portable runtimes
- **Agent tools**: code execution (`run_command`) and directory listing (`list_dir`) with multi-turn tool calling
- **Personas**: one-click System Prompt switching (coding expert / copywriter / data analyst / translator …)
- **First-run wizard**: two-step setup (API key + working directory)
- **Sci-fi theme**: deep-space neon palette, starfield background, glow effects, light/dark themes with font & density customization
- Conversation export (MD / JSON), full-text search, fully local storage, zero telemetry

## 🛠 Tech Stack

- **Shell**: Tauri 2 (Rust backend, ~10MB installer)
- **Frontend**: React 18 + TypeScript + Vite
- **Models**: DeepSeek official API + any OpenAI-compatible service

## 🚀 Development

### Prerequisites

1. **Node.js** ≥ 18 (tested with Node 22)
2. **Rust** via `rustup` (stable-msvc)
3. **Visual Studio Build Tools** with the "Desktop development with C++" workload (required for MSVC linking on Windows)

```bash
# Install dependencies
npm install

# Frontend-only preview (browser, no system capabilities)
npm run dev

# Full desktop app (dev mode, launches the Rust backend)
npm run tauri dev

# Build the installer
npm run tauri build
```

## 📥 Download & Install

Get the latest `DeepHarness_*.exe` from [Releases](https://github.com/yuntaojinghong/DeepHarness/releases) and double-click to install (installs to the user directory, no admin required, creates a desktop shortcut automatically).

> Prefer portable? Place an empty `portable.flag` file next to the app directory and data will follow the app — copy the whole folder to a USB stick and use it anywhere.

## 🎬 Getting Started

1. Follow the welcome wizard: enter your DeepSeek API Key (from platform.deepseek.com) and choose a working directory
2. Pick a model from the top dropdown (V3 / R1 …)
3. Start chatting; when the model supports tools, it can run commands and list directories for you

## 📁 Project Structure

```
deepseek-harness-desktop/
├── src/                    # React frontend
│   ├── App.tsx             # layout & theming
│   ├── store.ts            # zustand global state
│   ├── components/         # UI components
│   ├── lib/                # llm streaming client / env detection / storage
│   └── styles/global.css   # design system
└── src-tauri/              # Rust backend
    ├── src/lib.rs          # check_env / run_command / list_dir / storage / tray / notifications
    ├── tauri.conf.json
    └── Cargo.toml
```

## ⭐ Support

If you find this project useful, please give it a **Star ⭐** — your support keeps it going!

## 📄 License

[MIT](./LICENSE) © 2026 [yuntaojinghong](https://github.com/yuntaojinghong)

## 🤝 Contributing

Issues and pull requests are welcome. Please ensure `npm run build` (frontend) and `cargo check` (backend) pass before submitting.

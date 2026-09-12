/**
 * `.dph-plugin` 插件系统的前端契约层。
 *
 * 与 Rust 侧命令一一对应（`plugins::commands`）：
 * - `plugin_list` / `plugin_dir` / `plugin_install` / `plugin_set_enabled` /
 *   `plugin_uninstall`。
 *
 * ## 插件作用域
 *
 * 插件目录按 Agent 隔离（`agents/<agent>/plugins/`）。当前只有 DeepHarness
 * 自研循环会把插件工具挂给模型，所以界面只在 DeepHarness 面板里暴露入口 ——
 * 这是刻意为之，不是缺口：`.dph-plugin` **不与 dsh 的 cordis 插件生态
 * 兼容**，两者是各自独立的运行时。
 *
 * ## 安装为什么走 base64
 *
 * 前端没有任意路径的文件读取能力（本项目的安全前提），所以安装是
 * 「前端把文件读成 base64 → 后端解码解包」。后端**不接受路径参数**，
 * 因此不存在「诱导后端读任意文件」的攻击面。
 */

import { isTauri } from "./env";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

function assertTauri(what: string): void {
  if (!isTauri()) {
    throw new Error(`浏览器预览模式下不可用（${what}），请在桌面版中使用。`);
  }
}

/** 与 Rust 侧 `PluginRecord` 字段一一对应（camelCase）。 */
export interface PluginRecord {
  /** 目录名（= manifest.id）；manifest 损坏时取自目录名。 */
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  homepage: string;
  enabled: boolean;
  /** manifest 校验是否通过。 */
  valid: boolean;
  /** `valid === false` 时的可读原因。 */
  error: string | null;
  /** 声明的工具名列表（manifest 无效时为空）。 */
  tools: string[];
  /** 插件目录的绝对路径。 */
  dir: string;
}

/** 与 Rust 侧 `store::MAX_ARCHIVE_BYTES` 对齐（16 MB）。 */
export const MAX_PLUGIN_BYTES = 16 * 1024 * 1024;

export function pluginSizeLimitLabel(): string {
  return `${Math.round(MAX_PLUGIN_BYTES / 1024 / 1024)} MB`;
}

/** 列出某 Agent 已安装的插件（含损坏条目，便于用户看到并清理）。 */
export async function pluginList(agent: string): Promise<PluginRecord[]> {
  if (!isTauri()) return [];
  return invoke<PluginRecord[]>("plugin_list", { agent });
}

/** 该 Agent 的插件目录（用于提示用户手动放包）。 */
export async function pluginDir(agent: string): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("plugin_dir", { agent });
}

/**
 * 安装（或升级）一个插件包。
 *
 * 大文件走 base64 会膨胀 ~33%，所以前端先按体积挡一道，避免白读一遍文件。
 */
export async function pluginInstall(agent: string, file: File): Promise<PluginRecord> {
  assertTauri("安装插件");
  if (file.size === 0) {
    throw new Error("插件包是空文件。");
  }
  if (file.size > MAX_PLUGIN_BYTES) {
    throw new Error(`插件包过大（${formatBytes(file.size)}），上限 ${pluginSizeLimitLabel()}。`);
  }
  const archiveBase64 = await readFileAsBase64(file);
  return invoke<PluginRecord>("plugin_install", { agent, archiveBase64 });
}

/** 启用 / 停用（变更后若 Worker 在运行会立即热加载）。 */
export async function pluginSetEnabled(
  agent: string,
  id: string,
  enabled: boolean
): Promise<PluginRecord> {
  assertTauri("切换插件状态");
  return invoke<PluginRecord>("plugin_set_enabled", { agent, id, enabled });
}

/** 卸载（删除插件目录与状态项）。 */
export async function pluginUninstall(agent: string, id: string): Promise<void> {
  assertTauri("卸载插件");
  await invoke("plugin_uninstall", { agent, id });
}

/**
 * 把文件读成**纯** base64（不带 `data:…;base64,` 前缀）。
 *
 * 刻意不用 `readAsDataURL`：虽然后端会剥前缀，但少一层字符串处理就少一个
 * 出错点；而且大文件下前缀会让长度估算偏大。
 */
export function readFileAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`读取文件失败：${file.name}`));
    reader.onload = () => {
      const buf = reader.result;
      if (!(buf instanceof ArrayBuffer)) {
        reject(new Error("读取文件失败：结果不是二进制数据"));
        return;
      }
      const bytes = new Uint8Array(buf);
      // 分块拼接，避免超大文件下 `String.fromCharCode(...bytes)` 撑爆调用栈
      const CHUNK = 0x8000;
      let binary = "";
      for (let i = 0; i < bytes.length; i += CHUNK) {
        binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
      }
      resolve(btoa(binary));
    };
    reader.readAsArrayBuffer(file);
  });
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

// ---- 给用户的插件模板（界面里可直接复制） ----

export const PLUGIN_MANIFEST_EXAMPLE = `{
  "id": "com.example.hello",
  "name": "Hello 插件",
  "version": "1.0.0",
  "description": "演示插件：打招呼并统计工作区文件数",
  "author": "你的名字",
  "entry": "index.js",
  "tools": [
    {
      "name": "hello_report",
      "description": "根据 who 参数打招呼，并统计工作区文件数量",
      "argsSchema": "{ \\"who\\": string }"
    }
  ]
}`;

export const PLUGIN_ENTRY_EXAMPLE = `// index.js —— 必须导出 { tools: { 工具名: async (args, ctx) => 结果 } }
// ctx.readFile / ctx.writeFile / ctx.listDir 会回到宿主进程，
// 由该 Agent 的白名单逐次校验（插件本身被 node 沙箱锁在插件目录内）。
module.exports = {
  tools: {
    async hello_report(args, ctx) {
      const entries = await ctx.listDir(ctx.workspace);
      return {
        message: \`你好，\${args.who || "世界"}！\`,
        workspaceFiles: entries.filter((e) => !e.isDir).length,
      };
    },
  },
};`;

/** 组装一个可直接打包的示例插件（目录结构说明文本）。 */
export function pluginPackagingHint(): string {
  return [
    "打包方式：把 manifest.json 与 index.js 放进同一个文件夹，",
    "压缩为 zip（根目录直接是这两个文件，不要再套一层文件夹），",
    "把扩展名改为 .dph-plugin 后在上方安装。",
  ].join("");
}

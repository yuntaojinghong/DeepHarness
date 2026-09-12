import type { EnvStatus } from "../types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

export async function checkEnv(): Promise<EnvStatus> {
  if (isTauri()) {
    try {
      return await invoke<EnvStatus>("check_env");
    } catch {
      return browserFallback();
    }
  }
  return browserFallback();
}

function browserFallback(): EnvStatus {
  return {
    python: null,
    node: null,
    git: null,
    os: `${navigator.platform} (浏览器预览)`,
    arch: "—",
    dataDir: "浏览器预览模式 · 无应用目录",
    portable: false,
  };
}

export interface DirEntry {
  name: string;
  isDir: boolean;
  size: number;
}

/** 三个内置 Agent 的标识（与 Rust 侧 paths::AGENT_IDS 严格一致）。 */
export const AGENT_IDS = ["deepseek-harness", "codex", "deepharness"] as const;
export type AgentId = (typeof AGENT_IDS)[number];

/** 访问模式：只读 / 读写（与 Rust 侧 AccessMode 的 serde 序列化一致）。 */
export type AccessMode = "read" | "read_write";

export interface AccessDecision {
  granted: boolean;
  path: string;
}

export interface GrantView {
  path: string;
  mode: AccessMode;
  grantedAt: string;
  label: string | null;
}

export interface AgentDirsInfo {
  agentId: string;
  root: string;
  workspace: string;
  config: string;
  sessions: string;
  logs: string;
  plugins: string;
}

function assertTauri(): void {
  if (!isTauri()) {
    throw new Error("浏览器预览模式下不可用，请在桌面版中使用文件能力。");
  }
}

// ---- 权限管理 ----

/** 预检：路径是否已被授权（不弹窗、不落盘）。 */
export async function checkAccess(
  agent: AgentId,
  path: string,
  write: boolean
): Promise<AccessDecision> {
  assertTauri();
  return invoke<AccessDecision>("check_access", { agent, path, write });
}

/** 用户确认后授予路径访问权。 */
export async function grantAccess(
  agent: AgentId,
  path: string,
  mode: AccessMode,
  label?: string
): Promise<GrantView> {
  assertTauri();
  return invoke<GrantView>("grant_access", { agent, path, mode, label: label ?? null });
}

/** 撤销一条授权。 */
export async function revokeAccess(agent: AgentId, path: string): Promise<void> {
  assertTauri();
  await invoke("revoke_access", { agent, path });
}

/** 列出某 Agent 的全部授权（设置页展示与撤销用）。 */
export async function listGrants(agent: AgentId): Promise<GrantView[]> {
  assertTauri();
  return invoke<GrantView[]>("list_grants", { agent });
}

// ---- 文件操作（全部经过 Rust 侧白名单校验） ----

export async function readTextFile(agent: AgentId, path: string): Promise<string> {
  assertTauri();
  return invoke<string>("read_text_file", { agent, path });
}

export async function readFileBase64(agent: AgentId, path: string): Promise<string> {
  assertTauri();
  return invoke<string>("read_file_base64", { agent, path });
}

export async function writeTextFile(
  agent: AgentId,
  path: string,
  contents: string
): Promise<number> {
  assertTauri();
  return invoke<number>("write_text_file", { agent, path, contents });
}

export async function listDir(agent: AgentId, path: string): Promise<DirEntry[]> {
  if (isTauri()) {
    return invoke<DirEntry[]>("list_directory", { agent, path });
  }
  return [{ name: "（预览模式：无法读取目录，桌面版可真实列出）", isDir: false, size: 0 }];
}

export async function createDirectory(agent: AgentId, path: string): Promise<string> {
  assertTauri();
  return invoke<string>("create_directory", { agent, path });
}

export async function deletePath(agent: AgentId, path: string): Promise<void> {
  assertTauri();
  await invoke("delete_path", { agent, path });
}

export async function agentDirsInfo(agent: AgentId): Promise<AgentDirsInfo> {
  assertTauri();
  return invoke<AgentDirsInfo>("agent_dirs_info", { agent });
}

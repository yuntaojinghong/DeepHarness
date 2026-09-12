/**
 * 三个内置 Agent 的 IPC 契约层。
 *
 * 本模块是前端与 Rust 侧命令的唯一交汇点：
 * - `agent_*` 命令来自 `agents::registry`，负责统一启停与状态查询；
 * - `deepharness_*` 命令来自 `agents::registry`，仅服务自研原生 Agent
 *   （规划 / 执行 / 记忆），其余两个 Agent 不具备这些能力。
 *
 * 所有类型均与 Rust 侧 `#[serde(rename_all = "camelCase")]` 一一对应，
 * 字段改名时必须同步两侧，否则会在运行期静默得到 `undefined`。
 */

import { isTauri, type AgentId } from "./env";

// ---- 通用 invoke ----

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

function assertTauri(what: string): void {
  if (!isTauri()) {
    throw new Error(`浏览器预览模式下不可用（${what}），请在桌面版中使用。`);
  }
}

// ---- Agent 元信息 ----

export interface AgentMeta {
  /** 与 Rust 侧 `AgentRuntime::display_name` 一致。 */
  name: string;
  /** 一句话定位，用于切换器与空状态。 */
  tagline: string;
  /** 该 Agent 是否具备任务控制台（规划 / 执行 / 记忆）能力。 */
  hasTaskConsole: boolean;
  /**
   * 官方 Web UI 的**基础地址**，仅用于展示与「该 Agent 有独立界面」判断。
   *
   * 不要直接打开它：dsh 0.1.5 起需要每次启动重新生成的一次性 token，
   * 不带 token 访问只会拿到 401。真正打开请走 `agentOpenWebUi()`。
   */
  webUiUrl?: string;
}

export const AGENT_META: Record<AgentId, AgentMeta> = {
  "deepseek-harness": {
    name: "DeepSeek Harness",
    tagline: "官方 Harness 运行时，内置对话与工具链",
    hasTaskConsole: false,
    webUiUrl: "http://127.0.0.1:3080",
  },
  codex: {
    name: "Codex",
    tagline: "按需拉起的编码 Agent，探测本机 CLI 可用性",
    hasTaskConsole: false,
  },
  deepharness: {
    name: "DeepHarness",
    tagline: "自研原生 Agent：规划 → 执行 → 反思 → 记忆沉淀",
    hasTaskConsole: true,
  },
};

// ---- Agent 启停与状态 ----

/** Rust 侧 `AgentStatus` 的四种状态（内部标签枚举，tag = "state"）。 */
export type AgentStateName = "stopped" | "starting" | "running" | "crashed";

export interface AgentStatusView {
  state: AgentStateName;
  /** 仅 `state === "crashed"` 时存在。 */
  reason?: string;
}

export interface AgentOverview {
  id: string;
  displayName: string;
  status: AgentStatusView;
}

/** 中文状态文案。 */
export function agentStateLabel(status: AgentStatusView | undefined): string {
  if (!status) return "未知";
  switch (status.state) {
    case "running":
      return "运行中";
    case "starting":
      return "启动中";
    case "crashed":
      return "已崩溃";
    case "stopped":
      return "已停止";
    default:
      return "未知";
  }
}

/** 状态对应的徽标样式类（复用 global.css 的 .badge-dot）。 */
export function agentStateTone(status: AgentStatusView | undefined): string {
  if (!status) return "";
  switch (status.state) {
    case "running":
      return "ok";
    case "starting":
      return "busy";
    case "crashed":
      return "err";
    case "stopped":
      return "";
    default:
      return "";
  }
}

export async function agentList(): Promise<AgentOverview[]> {
  if (!isTauri()) return previewOverview();
  return invoke<AgentOverview[]>("agent_list");
}

export async function agentStart(agent: AgentId): Promise<void> {
  assertTauri("启动 Agent");
  await invoke("agent_start", { agent });
}

export async function agentStop(agent: AgentId): Promise<void> {
  assertTauri("停止 Agent");
  await invoke("agent_stop", { agent });
}

export async function agentStatus(agent: AgentId): Promise<AgentStatusView> {
  if (!isTauri()) return { state: "stopped" };
  return invoke<AgentStatusView>("agent_status", { agent });
}

/**
 * 用系统默认浏览器打开某个 Agent 的官方 Web UI。
 *
 * 由后端完成打开动作，原因有两条：
 * 1. dsh 0.1.5 起 Web UI 需要一次性 token，且该 token 要等它启动几秒后
 *    才打印出来 —— 前端「await 拿到地址再 window.open」既可能被弹窗
 *    拦截，也可能落到一个拿不到句柄的新窗口上；
 * 2. token 是本机临时凭据，留在 Rust 侧不流向前端。
 *
 * 返回**脱敏**后的地址（token 显示为 `***`），可直接用于提示文案。
 * 未运行或地址尚未就绪时抛错（有界等待后给出明确原因）。
 */
export async function agentOpenWebUi(agent: AgentId): Promise<string> {
  assertTauri("打开官方 Web UI");
  return invoke<string>("agent_open_web_ui", { agent });
}

/** 浏览器预览下给出三个「已停止」的占位概览，保证界面可完整渲染。 */
function previewOverview(): AgentOverview[] {
  return (Object.keys(AGENT_META) as AgentId[]).map((id) => ({
    id,
    displayName: AGENT_META[id].name,
    status: { state: "stopped" as const },
  }));
}

// ---- DeepHarness Worker 就绪状态 ----

export interface WorkerReadiness {
  running: boolean;
  configured: boolean;
  /** Worker 进程号；未运行时为 undefined。 */
  pid?: number | null;
  /** Worker 上报的协议版本。 */
  version?: string | null;
}

export async function deepharnessStatus(): Promise<WorkerReadiness> {
  if (!isTauri()) return { running: false, configured: false };
  return invoke<WorkerReadiness>("deepharness_status");
}

// ---- 模型配置 ----

export interface DeepHarnessConfigView {
  baseUrl: string | null;
  model: string | null;
  /** 仅为便于确认是哪把 Key，返回尾部 4 位。 */
  apiKeyTail: string | null;
  hasApiKey: boolean;
}

export interface ConfigureResult {
  saved: boolean;
  /** 是否已热注入到运行中的 Worker（未运行时为 false，下次启动自动生效）。 */
  applied: boolean;
  model: string;
}

export async function deepharnessGetConfig(): Promise<DeepHarnessConfigView> {
  if (!isTauri()) {
    return { baseUrl: null, model: null, apiKeyTail: null, hasApiKey: false };
  }
  return invoke<DeepHarnessConfigView>("deepharness_get_config");
}

export async function deepharnessConfigure(
  baseUrl: string,
  apiKey: string,
  model: string
): Promise<ConfigureResult> {
  assertTauri("配置模型");
  return invoke<ConfigureResult>("deepharness_configure", { baseUrl, apiKey, model });
}

// ---- 计划与任务 ----

export interface PlanStep {
  title: string;
  /** `null` 表示纯回答 / 总结步骤。 */
  tool: string | null;
  /** 工具参数；回答步骤可能为 null。 */
  args: unknown;
  /** 预期结果；回答步骤时为说明文本。 */
  expected: string;
}

export interface Plan {
  goal: string;
  steps: PlanStep[];
}

export interface StepReview {
  success: boolean;
  summary: string;
  /** 失败且值得原样重试时为 true。 */
  shouldRetry: boolean;
  /** 给规划器 / 用户的修正建议。 */
  advice: string;
}

export interface StepRecord {
  title: string;
  tool: string | null;
  ok: boolean;
  /** 工具原始结果（JSON 字符串）或错误信息。 */
  result: string;
  review: StepReview;
  /** 是否经历了一次重试。 */
  retried: boolean;
  /** 重试是否使用了规划器修订后的步骤。 */
  revised: boolean;
}

export interface TaskOutcome {
  goal: string;
  success: boolean;
  summary: string;
  steps: StepRecord[];
}

export async function deepharnessPlan(goal: string, context?: string): Promise<Plan> {
  assertTauri("生成计划");
  return invoke<Plan>("deepharness_plan", { goal, context: context ?? null });
}

export async function deepharnessRunTask(goal: string, context?: string): Promise<TaskOutcome> {
  assertTauri("执行任务");
  return invoke<TaskOutcome>("deepharness_run_task", { goal, context: context ?? null });
}

// ---- 长期记忆 ----

export interface Memory {
  id: number;
  kind: string;
  content: string;
  tags: string[];
  importance: number;
  createdAt: string;
  updatedAt: string;
}

export interface MemoryStats {
  total: number;
  /** [kind, 条数] 二元组列表（Rust 侧为 Vec<(String, i64)>）。 */
  byKind: [string, number][];
}

/** 与 Rust 侧 `native::memory::MEMORY_KINDS` 严格一致。 */
export const MEMORY_KINDS = ["fact", "preference", "event", "reflection"] as const;
export type MemoryKind = (typeof MEMORY_KINDS)[number];

const MEMORY_KIND_LABELS: Record<string, string> = {
  fact: "事实",
  preference: "偏好",
  event: "事件",
  reflection: "反思",
};

export function memoryKindLabel(kind: string): string {
  return MEMORY_KIND_LABELS[kind] ?? kind;
}

export async function deepharnessRecall(query: string, limit = 5): Promise<Memory[]> {
  assertTauri("检索记忆");
  return invoke<Memory[]>("deepharness_recall", { query, limit });
}

export async function deepharnessRemember(
  kind: MemoryKind,
  content: string,
  tags: string[] = [],
  importance = 0.5
): Promise<number> {
  assertTauri("写入记忆");
  return invoke<number>("deepharness_remember", { kind, content, tags, importance });
}

/** 返回该 id 是否曾经存在。 */
export async function deepharnessForget(id: number): Promise<boolean> {
  assertTauri("删除记忆");
  return invoke<boolean>("deepharness_forget", { id });
}

export async function deepharnessMemoryStats(): Promise<MemoryStats> {
  assertTauri("读取记忆统计");
  return invoke<MemoryStats>("deepharness_memory_stats");
}

// ---- 展示辅助 ----

/** 把工具结果截断到可读长度，避免超长 JSON 撑爆界面。 */
export function truncateResult(result: string, max = 600): string {
  if (result.length <= max) return result;
  return `${result.slice(0, max)}\n…（已截断，共 ${result.length} 字符）`;
}

/** 把 `args` 渲染为紧凑单行 JSON。 */
export function formatArgs(args: unknown): string {
  if (args === null || args === undefined) return "";
  try {
    return JSON.stringify(args);
  } catch {
    return String(args);
  }
}

/**
 * 统一把异常转成可展示的中文文案。
 *
 * Tauri 命令失败时抛出的通常是字符串（而不是 Error），直接 String(e)
 * 会丢掉边界情况下的可读性，这里一并处理并保留原始信息。
 */
export function describeError(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    try {
      return JSON.stringify(e);
    } catch {
      /* 落到 String 分支 */
    }
  }
  return String(e);
}

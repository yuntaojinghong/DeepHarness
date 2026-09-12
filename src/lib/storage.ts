import type { AppSettings, Conversation, ModelConfig, Persona } from "../types";
import { AGENT_IDS, type AgentId } from "./env";

const K_CONVERSATIONS = "dh.conversations";
const K_SETTINGS = "dh.settings";
const K_MODELS = "dh.models";
const K_OFFICIAL_MODELS = "dh.officialModels";
const K_SELECTED = "dh.selected";
const K_ACTIVE_AGENT = "dh.activeAgent";

export function uid(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 会话默认归属的 Agent（历史数据未记录 agent 字段时使用）。 */
export const DEFAULT_AGENT_ID = "deepseek-harness";

/**
 * 补全历史会话缺失的字段。
 *
 * 旧版本的会话没有 `agent` 字段，直接读取会得到 `undefined`，
 * 导致按 Agent 过滤时会话「凭空消失」。这里统一兜底到默认 Agent。
 */
export function normalizeConversation(raw: Partial<Conversation>): Conversation {
  return {
    id: raw.id ?? uid(),
    title: raw.title ?? "新会话",
    messages: raw.messages ?? [],
    modelId: raw.modelId ?? DEFAULT_SETTINGS.defaultModelId,
    agent: raw.agent ?? DEFAULT_AGENT_ID,
    systemPromptId: raw.systemPromptId,
    workspace: raw.workspace,
    archived: raw.archived,
    createdAt: raw.createdAt ?? Date.now(),
    updatedAt: raw.updatedAt ?? Date.now(),
  };
}

export function loadConversations(): Conversation[] {
  try {
    const raw = JSON.parse(localStorage.getItem(K_CONVERSATIONS) || "[]") as Partial<Conversation>[];
    return raw.map(normalizeConversation);
  } catch {
    return [];
  }
}

export function saveConversations(list: Conversation[]) {
  localStorage.setItem(K_CONVERSATIONS, JSON.stringify(list));
}

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "dark",
  apiKeys: {},
  fontSize: 14,
  density: "comfortable",
  workspace: "",
  defaultModelId: "deepseek-v4-flash",
};

export function loadSettings(): AppSettings {
  try {
    return { ...DEFAULT_SETTINGS, ...JSON.parse(localStorage.getItem(K_SETTINGS) || "{}") };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(s: AppSettings) {
  localStorage.setItem(K_SETTINGS, JSON.stringify(s));
}

export const BUILTIN_MODELS: ModelConfig[] = [
  {
    id: "deepseek-v4-flash",
    name: "DeepSeek V4 Flash",
    provider: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-v4-flash",
    contextWindow: 1000000,
    supportsTools: true,
    builtin: true,
  },
  {
    id: "deepseek-v4-pro",
    name: "DeepSeek V4 Pro",
    provider: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-v4-pro",
    contextWindow: 1000000,
    supportsTools: true,
    builtin: true,
  },
];

export function dedupeModels(...groups: ModelConfig[][]): ModelConfig[] {
  const seen = new Set<string>();
  const out: ModelConfig[] = [];
  for (const group of groups) {
    for (const m of group) {
      if (seen.has(m.id)) continue;
      seen.add(m.id);
      out.push(m);
    }
  }
  return out;
}

export function loadCustomModels(): ModelConfig[] {
  try {
    return JSON.parse(localStorage.getItem(K_MODELS) || "[]") as ModelConfig[];
  } catch {
    return [];
  }
}

export function saveCustomModels(list: ModelConfig[]) {
  localStorage.setItem(K_MODELS, JSON.stringify(list));
}

export function loadOfficialModels(): ModelConfig[] {
  try {
    return JSON.parse(localStorage.getItem(K_OFFICIAL_MODELS) || "[]") as ModelConfig[];
  } catch {
    return [];
  }
}

export function saveOfficialModels(list: ModelConfig[]) {
  localStorage.setItem(K_OFFICIAL_MODELS, JSON.stringify(list));
}

export function loadModels(): ModelConfig[] {
  return dedupeModels(BUILTIN_MODELS, loadOfficialModels(), loadCustomModels());
}

export function saveModels(list: ModelConfig[]) {
  const custom = list.filter((m) => !m.builtin);
  localStorage.setItem(K_MODELS, JSON.stringify(custom));
}

export function loadSelected(): string {
  return localStorage.getItem(K_SELECTED) || "deepseek-v4-flash";
}

export function saveSelected(id: string) {
  localStorage.setItem(K_SELECTED, id);
}

/** 读取当前激活的 Agent；非法或缺失时回退到默认 Agent。 */
export function loadActiveAgent(): AgentId {
  try {
    const raw = localStorage.getItem(K_ACTIVE_AGENT);
    if (raw && (AGENT_IDS as readonly string[]).includes(raw)) return raw as AgentId;
  } catch {
    /* localStorage 不可用时按默认处理 */
  }
  return DEFAULT_AGENT_ID as AgentId;
}

export function saveActiveAgent(id: string) {
  try {
    localStorage.setItem(K_ACTIVE_AGENT, id);
  } catch {
    /* ignore */
  }
}

export function exportConversationJson(conv: Conversation) {
  const blob = new Blob([JSON.stringify(conv, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `${conv.title || "conversation"}.json`;
  a.click();
  URL.revokeObjectURL(a.href);
}

export function exportConversationMarkdown(conv: Conversation) {
  const lines: string[] = [`# ${conv.title || "对话记录"}`, `> 模型: ${conv.modelId} · 导出时间: ${new Date().toLocaleString()}`, ""];
  for (const m of conv.messages) {
    if (m.role === "user") lines.push(`## 用户\n\n${m.content}\n`);
    else if (m.role === "assistant") lines.push(`## AI\n\n${m.content}\n`);
  }
  const blob = new Blob([lines.join("\n")], { type: "text/markdown" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `${conv.title || "conversation"}.md`;
  a.click();
  URL.revokeObjectURL(a.href);
}

export const PERSONAS: Persona[] = [
  {
    id: "default",
    name: "通用助手",
    color: "#22d3ee",
    prompt: "",
  },
  {
    id: "coder",
    name: "编程专家",
    color: "#4d6bfe",
    prompt: "你是一名资深软件工程师。回答技术问题时给出可直接运行的代码、解释关键实现，并指出常见的坑。代码优先，简洁专业。",
  },
  {
    id: "writer",
    name: "中文文案",
    color: "#8b5cf6",
    prompt: "你是一名资深中文文案策划。文笔流畅、有感染力，擅长润色、起标题、写营销文案与公众号文章。输出直接可用，不堆砌套话。",
  },
  {
    id: "analyst",
    name: "数据分析",
    color: "#10b981",
    prompt: "你是一名数据分析师。对数据先给结论，再给依据；能用表格和步骤清晰呈现分析过程，并指出数据的局限与可优化的方向。",
  },
  {
    id: "translator",
    name: "翻译官",
    color: "#f59e0b",
    prompt: "你是一名专业翻译。中英互译准确、地道，保留原文语气与专业术语。默认只输出译文，需要时附上术语说明。",
  },
];

export interface DiskData {
  conversations: Conversation[];
  settings: AppSettings;
  models: ModelConfig[];
  officialModels: ModelConfig[];
  selected: string;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function readDiskData(): Promise<DiskData | null> {
  if (!isTauri()) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const raw = await invoke<string>("read_store", { key: "data" });
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<DiskData>;
    const official = parsed.officialModels ?? [];
    return {
      conversations: (parsed.conversations ?? []).map(normalizeConversation),
      settings: { ...DEFAULT_SETTINGS, ...(parsed.settings ?? {}) },
      models: dedupeModels(BUILTIN_MODELS, official, (parsed.models ?? []).filter((m) => !m.builtin)),
      officialModels: official,
      selected: parsed.selected ?? "deepseek-v4-flash",
    };
  } catch {
    return null;
  }
}

export async function writeDiskData(data: DiskData) {
  if (!isTauri()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("write_store", {
      key: "data",
      value: JSON.stringify({
        conversations: data.conversations,
        settings: data.settings,
        models: data.models.filter((m) => !m.builtin),
        officialModels: data.officialModels,
        selected: data.selected,
      }),
    });
  } catch {
    /* ignore */
  }
}

export async function notify(title: string, body: string) {
  if (!isTauri()) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("notify", { title, body });
  } catch {
    /* ignore */
  }
}

import { create } from "zustand";
import type { AppSettings, Conversation, EnvStatus, ModelConfig, ToolToggle } from "./types";
import {
  loadConversations,
  loadModels,
  loadOfficialModels,
  loadSelected,
  loadSettings,
  saveConversations,
  saveCustomModels,
  saveModels,
  saveOfficialModels,
  saveSelected,
  saveSettings,
  saveActiveAgent,
  loadActiveAgent,
  uid,
  readDiskData,
  writeDiskData,
  BUILTIN_MODELS,
  dedupeModels,
} from "./lib/storage";
import { fetchOfficialModels } from "./lib/models";
import { AGENT_IDS, type AgentId } from "./lib/env";
import {
  agentList,
  deepharnessStatus,
  type AgentOverview,
  type AgentStatusView,
  type WorkerReadiness,
} from "./lib/deepharness";
import { conversationsOf, filterConversations } from "./lib/conversations";

/** 启动时确定激活会话：优先上次选中项，其次该 Agent 的第一条。 */
function bootstrapActiveId(list: Conversation[], agent: AgentId): string | null {
  const mine = conversationsOf(list, agent);
  const selected = loadSelected();
  if (mine.some((c) => c.id === selected)) return selected;
  return mine[0]?.id ?? null;
}

interface AppState {
  conversations: Conversation[];
  activeId: string | null;
  /** 当前激活的 Agent；三个 Agent 的界面与会话完全隔离。 */
  activeAgent: AgentId;
  /** 三个 Agent 的运行时概览（状态由 Rust 注册表上报）。 */
  agents: AgentOverview[];
  agentsRefreshing: boolean;
  agentsError: string | null;
  /**
   * DeepHarness Worker 的就绪状态（唯一来源）。
   *
   * 任务控制台与右侧面板都从这里读，而不是各自轮询——否则同一个
   * `deepharness_status` 会被并发调用两次，且两处状态可能短暂不一致。
   */
  workerReadiness: WorkerReadiness | null;
  settings: AppSettings;
  models: ModelConfig[];
  officialModels: ModelConfig[];
  modelsRefreshing: boolean;
  env: EnvStatus | null;
  envChecking: boolean;
  tools: ToolToggle;
  sidebarOpen: boolean;
  contextOpen: boolean;
  settingsOpen: boolean;
  envOpen: boolean;
  welcomeOpen: boolean;
  supportOpen: boolean;
  searchQuery: string;
  hydrated: boolean;

  // ── 查询方法 ──────────────────────────────────────────────────────────
  // 这些方法是给 **store 内部动作**（setPersona / selectModel 等）和普通函数
  // 用的。其中 activeConversation / searchConversations / agentStatus 会返回
  // 新引用或派生值，**不可**作为 `useAppStore(s => ...)` 的选择器返回值：
  // zustand v5 用 Object.is 比较快照，新引用会让 React 无限重渲染
  // （React #185），整棵树被卸载，界面只剩一片深色底（黑屏）。
  // 组件侧请改用 src/lib/hooks.ts 里的 useActiveConversation /
  // useVisibleConversations / useAgentStatus。
  activeConversation: () => Conversation | null;
  hydrate: () => Promise<void>;
  setActive: (id: string) => void;
  setActiveAgent: (agent: AgentId) => void;
  agentOverview: (agent: AgentId) => AgentOverview | null;
  agentStatus: (agent: AgentId) => AgentStatusView | undefined;
  refreshAgents: () => Promise<void>;
  refreshWorkerReadiness: () => Promise<WorkerReadiness>;
  newConversation: (modelId?: string) => string;
  renameConversation: (id: string, title: string) => void;
  deleteConversation: (id: string) => void;
  updateConversation: (id: string, fn: (c: Conversation) => Conversation) => void;
  searchConversations: () => Conversation[];
  setPersona: (personaId: string) => void;

  setSettings: (s: Partial<AppSettings>) => void;
  setApiKey: (provider: string, key: string) => void;
  addModel: (m: ModelConfig) => void;
  removeModel: (id: string) => void;
  selectModel: (id: string) => void;
  refreshModels: () => Promise<void>;

  setEnv: (e: EnvStatus | null) => void;
  setEnvChecking: (v: boolean) => void;
  setTools: (t: Partial<ToolToggle>) => void;
  setSidebarOpen: (v: boolean) => void;
  setContextOpen: (v: boolean) => void;
  setSettingsOpen: (v: boolean) => void;
  setEnvOpen: (v: boolean) => void;
  setWelcomeOpen: (v: boolean) => void;
  setSupportOpen: (v: boolean) => void;
  setSearchQuery: (q: string) => void;
}

export const useAppStore = create<AppState>((set, get) => {
  const bootConversations = loadConversations();
  const bootAgent = loadActiveAgent();
  return {
  conversations: bootConversations,
  activeId: bootstrapActiveId(bootConversations, bootAgent),
  activeAgent: bootAgent,
  agents: [],
  agentsRefreshing: false,
  agentsError: null,
  workerReadiness: null,
  settings: loadSettings(),
  models: loadModels(),
  officialModels: loadOfficialModels(),
  modelsRefreshing: false,
  env: null,
  envChecking: false,
  tools: { fileTools: true },
  sidebarOpen: true,
  contextOpen: true,
  settingsOpen: false,
  envOpen: false,
  welcomeOpen: false,
  supportOpen: false,
  searchQuery: "",
  hydrated: false,

  activeConversation: () => {
    const s = get();
    const found = s.conversations.find((c) => c.id === s.activeId) ?? null;
    // 只返回属于当前 Agent 的会话，避免切换后残留上一个 Agent 的内容。
    return found && found.agent === s.activeAgent ? found : null;
  },

  hydrate: async () => {
    const disk = await readDiskData();
    if (disk) {
      saveConversations(disk.conversations);
      saveSettings(disk.settings);
      saveModels(disk.models);
      saveOfficialModels(disk.officialModels);
      saveSelected(disk.selected);
      const agent = get().activeAgent;
      const mine = conversationsOf(disk.conversations, agent);
      set({
        conversations: disk.conversations,
        settings: disk.settings,
        models: disk.models,
        officialModels: disk.officialModels,
        activeId: mine.some((c) => c.id === disk.selected)
          ? disk.selected
          : mine[0]?.id ?? null,
        hydrated: true,
      });
    } else {
      set({ hydrated: true });
    }
  },

  setPersona: (personaId) => {
    const conv = get().activeConversation();
    if (conv) {
      get().updateConversation(conv.id, (c) => ({ ...c, systemPromptId: personaId }));
    }
  },

  setActive: (id) => {
    saveSelected(id);
    set({ activeId: id });
  },

  setActiveAgent: (agent) => {
    if (!(AGENT_IDS as readonly string[]).includes(agent)) return;
    const s = get();
    if (s.activeAgent === agent) return;
    saveActiveAgent(agent);
    // 切换 Agent 时把激活会话切换到该 Agent 名下，不跨 Agent 泄漏内容。
    const mine = conversationsOf(s.conversations, agent);
    const selected = loadSelected();
    const nextId = mine.some((c) => c.id === selected) ? selected : mine[0]?.id ?? null;
    if (nextId) saveSelected(nextId);
    set({ activeAgent: agent, activeId: nextId, searchQuery: "" });
    // 状态徽标随切换即时刷新（失败不阻塞界面）。
    void get().refreshAgents();
  },

  agentOverview: (agent) => get().agents.find((a) => a.id === agent) ?? null,

  agentStatus: (agent) => get().agents.find((a) => a.id === agent)?.status,

  refreshAgents: async () => {
    if (get().agentsRefreshing) return;
    set({ agentsRefreshing: true });
    try {
      const list = await agentList();
      // 只保留已知 Agent，防止后端新增 ID 时污染界面状态。
      set({
        agents: list.filter((a) => (AGENT_IDS as readonly string[]).includes(a.id)),
        agentsError: null,
      });
    } catch (e) {
      set({ agentsError: e instanceof Error ? e.message : String(e) });
    } finally {
      set({ agentsRefreshing: false });
    }
  },

  refreshWorkerReadiness: async () => {
    try {
      const readiness = await deepharnessStatus();
      set({ workerReadiness: readiness });
      return readiness;
    } catch (e) {
      // 读取失败不能当成 Worker 正常：按「未运行且未配置」保守处理，
      // 并让调用方拿到同一个值，避免界面出现两套结论。
      console.warn("[deepharness] 读取 Worker 状态失败", e);
      const fallback: WorkerReadiness = { running: false, configured: false };
      set({ workerReadiness: fallback });
      return fallback;
    }
  },

  newConversation: (modelId) => {
    const s = get();
    const id = uid();
    const conv: Conversation = {
      id,
      title: "新会话",
      messages: [],
      modelId: modelId || s.settings.defaultModelId,
      agent: s.activeAgent,
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    const list = [conv, ...s.conversations];
    saveConversations(list);
    saveSelected(id);
    set({ conversations: list, activeId: id, searchQuery: "" });
    return id;
  },

  renameConversation: (id, title) => {
    set({
      conversations: get().conversations.map((c) => (c.id === id ? { ...c, title } : c)),
    });
  },

  deleteConversation: (id) => {
    const s = get();
    const list = s.conversations.filter((c) => c.id !== id);
    saveConversations(list);
    // 兜底会话必须落在同一个 Agent 名下。
    const mine = conversationsOf(list, s.activeAgent);
    const next = mine[0]?.id ?? null;
    if (next) saveSelected(next);
    set({ conversations: list, activeId: next });
  },

  updateConversation: (id, fn) => {
    set({
      conversations: get().conversations.map((c) => (c.id === id ? fn(c) : c)),
    });
  },

  searchConversations: () => {
    const s = get();
    return filterConversations(s.conversations, s.activeAgent, s.searchQuery);
  },

  setSettings: (partial) => {
    const next = { ...get().settings, ...partial };
    saveSettings(next);
    set({ settings: next });
  },

  setApiKey: (provider, key) => {
    const keys = { ...get().settings.apiKeys, [provider]: key };
    get().setSettings({ apiKeys: keys });
  },

  addModel: (m) => {
    const custom = [...get().models.filter((x) => !x.builtin), m];
    saveCustomModels(custom);
    set({ models: dedupeModels(BUILTIN_MODELS, get().officialModels, custom) });
  },

  removeModel: (id) => {
    const custom = get().models.filter((x) => x.id !== id && !x.builtin);
    saveCustomModels(custom);
    set({ models: dedupeModels(BUILTIN_MODELS, get().officialModels, custom) });
  },

  refreshModels: async () => {
    const key = (get().settings.apiKeys["deepseek"] || "").trim();
    if (!key || get().modelsRefreshing) return;
    set({ modelsRefreshing: true });
    try {
      const official = await fetchOfficialModels("https://api.deepseek.com/v1", key);
      saveOfficialModels(official);
      const custom = get().models.filter((m) => !m.builtin);
      set({
        officialModels: official,
        models: dedupeModels(BUILTIN_MODELS, official, custom),
      });
    } catch {
      /* 拉取失败则静默，保留内置与缓存 */
    } finally {
      set({ modelsRefreshing: false });
    }
  },

  selectModel: (id) => {
    get().setSettings({ defaultModelId: id });
    const conv = get().activeConversation();
    if (conv) {
      get().updateConversation(conv.id, (c) => ({ ...c, modelId: id }));
    }
  },

  setEnv: (e) => set({ env: e }),
  setEnvChecking: (v) => set({ envChecking: v }),
  setTools: (t) => set({ tools: { ...get().tools, ...t } }),
  setSidebarOpen: (v) => set({ sidebarOpen: v }),
  setContextOpen: (v) => set({ contextOpen: v }),
  setSettingsOpen: (v) => set({ settingsOpen: v }),
  setEnvOpen: (v) => set({ envOpen: v }),
  setWelcomeOpen: (v) => set({ welcomeOpen: v }),
  setSupportOpen: (v) => set({ supportOpen: v }),
  setSearchQuery: (q) => set({ searchQuery: q }),
  };
});

let persistTimer: ReturnType<typeof setTimeout> | null = null;
useAppStore.subscribe((state) => {
  if (!state.hydrated) return;
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    writeDiskData({
      conversations: state.conversations,
      settings: state.settings,
      models: state.models,
      officialModels: state.officialModels,
      selected: state.activeId ?? state.settings.defaultModelId,
    });
  }, 400);
});

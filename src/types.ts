export type Role = "user" | "assistant" | "system";

export interface ToolCallRecord {
  name: string;
  args: string;
  result: string;
  ok: boolean;
}

export interface ChatMessage {
  id: string;
  role: Role;
  content: string;
  model?: string;
  toolCalls?: ToolCallRecord[];
  createdAt: number;
  error?: boolean;
  streaming?: boolean;
}

export interface Conversation {
  id: string;
  title: string;
  messages: ChatMessage[];
  modelId: string;
  /**
   * 该会话归属的 Agent（与 Rust 侧 paths::AGENT_IDS 一致）。
   * 会话、工作区、授权、日志均按 Agent 隔离，切换 Agent 时只看到自己的会话。
   * 历史数据可能缺省，读取时按 `deepseek-harness` 兜底。
   */
  agent: string;
  systemPromptId?: string;
  workspace?: string;
  archived?: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface ModelConfig {
  id: string;
  name: string;
  provider: "deepseek" | "openai" | "custom";
  baseUrl: string;
  model: string;
  contextWindow: number;
  supportsTools: boolean;
  temperature?: number;
  maxTokens?: number;
  builtin?: boolean;
}

export interface AppSettings {
  theme: "light" | "dark" | "system";
  apiKeys: Record<string, string>;
  fontSize: number;
  density: "comfortable" | "compact";
  workspace: string;
  defaultModelId: string;
}

export interface EnvStatus {
  python: { installed: boolean; version?: string; path?: string } | null;
  node: { installed: boolean; version?: string; path?: string } | null;
  git: { installed: boolean; version?: string; path?: string } | null;
  os: string;
  arch: string;
  dataDir: string;
  portable: boolean;
}

export interface Persona {
  id: string;
  name: string;
  color: string;
  prompt: string;
}

export interface ToolToggle {
  code: boolean;
  file: boolean;
  search: boolean;
}

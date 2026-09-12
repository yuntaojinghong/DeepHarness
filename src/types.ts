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

/**
 * 对话路径的工具开关。
 *
 * 只保留**真实存在**的能力开关：内置工具集目前就是 list_dir / read_file / write_file
 * 三个文件类工具（见 `lib/agent-tools.ts`），因此这里只有一个字段。
 * 曾经多出的 `code`（代码执行）与 `search`（网页搜索）没有对应工具，
 * 拨动它们只会连带影响同一个布尔值，属于误导性界面，已移除。
 *
 * 不提供命令执行与网络访问是**刻意的安全边界**，不是待办事项。
 */
export interface ToolToggle {
  /** 文件工具（list_dir / read_file / write_file），逐次经权限白名单校验。 */
  fileTools: boolean;
}

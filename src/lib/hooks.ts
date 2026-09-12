import { useMemo } from "react";
import { useAppStore } from "../store";
import type { Conversation } from "../types";
import type { AgentId } from "./env";
import type { AgentStatusView } from "./deepharness";
import { filterConversations } from "./conversations";

/**
 * 从 store 派生数据的只读钩子。
 *
 * 为什么需要这一层：
 * zustand v5 的 `useStore` 用 `Object.is` 比较「选择器返回值」。选择器一旦
 * 返回**新引用**（数组、对象、或调用一个会新建结果的 store 方法），React 每次
 * 拿到的快照都不相等，就会不停重渲染，最终抛 React #185
 * 「Maximum update depth exceeded」并卸载整棵组件树——用户看到的就是黑屏。
 *
 * 因此本文件统一遵守一条规则：
 *   **选择器只取状态切片（state slice），一切派生计算放进 `useMemo`。**
 * 组件不要再写 `useAppStore((s) => s.某个方法())`；`src/lib/selector-guard.test.ts`
 * 会静态扫描并拦住这类写法。
 */

/** 当前激活的会话；不属于当前 Agent 时返回 null（切换 Agent 不串会话）。 */
export function useActiveConversation(): Conversation | null {
  const conversations = useAppStore((s) => s.conversations);
  const activeId = useAppStore((s) => s.activeId);
  const activeAgent = useAppStore((s) => s.activeAgent);

  return useMemo(() => {
    const found = conversations.find((c) => c.id === activeId) ?? null;
    return found && found.agent === activeAgent ? found : null;
  }, [conversations, activeId, activeAgent]);
}

/** 当前 Agent 名下、且命中侧栏搜索词的会话列表。 */
export function useVisibleConversations(): Conversation[] {
  const conversations = useAppStore((s) => s.conversations);
  const activeAgent = useAppStore((s) => s.activeAgent);
  const searchQuery = useAppStore((s) => s.searchQuery);

  return useMemo(
    () => filterConversations(conversations, activeAgent, searchQuery),
    [conversations, activeAgent, searchQuery],
  );
}

/** 某个 Agent 的运行时状态；未在概览里出现时为 undefined。 */
export function useAgentStatus(agent: AgentId): AgentStatusView | undefined {
  const agents = useAppStore((s) => s.agents);

  return useMemo(() => agents.find((a) => a.id === agent)?.status, [agents, agent]);
}

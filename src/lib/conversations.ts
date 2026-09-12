import type { Conversation } from "../types";
import type { AgentId } from "./env";

/**
 * 某个 Agent 名下的会话（保持既有顺序）。
 *
 * ⚠️ 本函数**每次调用都返回新数组**（`Array.prototype.filter` 的语义）。
 * 因此它只能出现在普通函数或 `useMemo` 里，**严禁**直接塞进 zustand 选择器：
 * 选择器返回值会被 `useSyncExternalStore` 用 `Object.is` 比较，新引用导致
 * 快照恒不相等 → 无限重渲染 → React 报 #185「Maximum update depth exceeded」
 * → 整棵组件树被卸载（窗口只剩一片深色底，看起来就是「黑屏」）。
 */
export function conversationsOf(list: Conversation[], agent: AgentId): Conversation[] {
  return list.filter((c) => c.agent === agent);
}

/**
 * 在指定 Agent 的会话里按标题 / 消息正文做大小写无关的模糊搜索。
 *
 * 与 `conversationsOf` 同上：返回新数组，不可用作选择器返回值。
 * 组件侧请用 `useVisibleConversations()`（内部已 `useMemo`）。
 */
export function filterConversations(
  list: Conversation[],
  agent: AgentId,
  query: string,
): Conversation[] {
  const mine = conversationsOf(list, agent);
  const q = query.trim().toLowerCase();
  if (!q) return mine;
  return mine.filter((c) => {
    if (c.title.toLowerCase().includes(q)) return true;
    return c.messages.some((m) => m.content.toLowerCase().includes(q));
  });
}

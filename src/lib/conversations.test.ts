import { describe, expect, it } from "vitest";
import type { Conversation } from "../types";
import { conversationsOf, filterConversations } from "./conversations";

/** 造一条最小可用的会话，只覆盖断言语义涉及的字段。 */
function conv(over: Partial<Conversation> & { id: string }): Conversation {
  return {
    title: "新会话",
    messages: [],
    modelId: "deepseek-v4-flash",
    agent: "deepseek-harness",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

const LIST: Conversation[] = [
  conv({ id: "a1", agent: "deepseek-harness", title: "整理周报", updatedAt: 3 }),
  conv({ id: "b1", agent: "codex", title: "重构构建脚本", updatedAt: 2 }),
  conv({ id: "b2", agent: "codex", title: "Fix Build Script", updatedAt: 2 }),
  conv({
    id: "a2",
    agent: "deepseek-harness",
    title: "写单元测试",
    updatedAt: 1,
    messages: [
      { id: "m1", role: "user", content: "帮我看看 Rust 侧的权限白名单", createdAt: 1 },
      { id: "m2", role: "assistant", content: "已按 Agent 拆成三份", createdAt: 2 },
    ],
  }),
  conv({ id: "c1", agent: "deepharness", title: "季度数据汇总" }),
];

describe("conversationsOf", () => {
  it("只保留指定 Agent 名下的会话", () => {
    expect(conversationsOf(LIST, "deepseek-harness").map((c) => c.id)).toEqual(["a1", "a2"]);
    expect(conversationsOf(LIST, "codex").map((c) => c.id)).toEqual(["b1", "b2"]);
    expect(conversationsOf(LIST, "deepharness").map((c) => c.id)).toEqual(["c1"]);
  });

  it("保持原有顺序", () => {
    const ids = conversationsOf(LIST, "deepseek-harness").map((c) => c.id);
    expect(ids).toEqual(["a1", "a2"]);
  });

  it("同一个 Agent 名下没有会话时返回空数组", () => {
    expect(conversationsOf([], "codex")).toEqual([]);
  });

  it("每次调用都返回新数组——这正是它不能作为 zustand 选择器的原因", () => {
    // 如果这条断言失败，说明实现被换成了缓存版本，可以放宽选择器约束；
    // 在此之前，src/lib/selector-guard.test.ts 的守卫都必须保持生效。
    const first = conversationsOf(LIST, "codex");
    const second = conversationsOf(LIST, "codex");
    expect(first).not.toBe(second);
    expect(first).toEqual(second);
  });
});

describe("filterConversations", () => {
  it("搜索词为空时返回该 Agent 的全部会话", () => {
    expect(filterConversations(LIST, "deepseek-harness", "").map((c) => c.id)).toEqual(["a1", "a2"]);
  });

  it("只有空白的搜索词等同于空搜索", () => {
    expect(filterConversations(LIST, "deepseek-harness", "   ").map((c) => c.id)).toEqual(["a1", "a2"]);
  });

  it("按标题命中，且大小写无关", () => {
    expect(filterConversations(LIST, "deepseek-harness", "周报").map((c) => c.id)).toEqual(["a1"]);
    expect(filterConversations(LIST, "codex", "构建").map((c) => c.id)).toEqual(["b1"]);
    expect(filterConversations(LIST, "codex", "build").map((c) => c.id)).toEqual(["b2"]);
    expect(filterConversations(LIST, "codex", "BUILD").map((c) => c.id)).toEqual(["b2"]);
    expect(filterConversations(LIST, "codex", "BuIlD").map((c) => c.id)).toEqual(["b2"]);
  });

  it("按消息正文命中", () => {
    expect(filterConversations(LIST, "deepseek-harness", "权限白名单").map((c) => c.id)).toEqual(["a2"]);
  });

  it("搜索词两侧空白会被裁剪", () => {
    expect(filterConversations(LIST, "deepseek-harness", "  周报  ").map((c) => c.id)).toEqual(["a1"]);
  });

  it("不会搜到别的 Agent 的会话", () => {
    expect(filterConversations(LIST, "deepseek-harness", "季度数据汇总")).toEqual([]);
  });

  it("没有命中时返回空数组", () => {
    expect(filterConversations(LIST, "deepseek-harness", "不存在的关键词")).toEqual([]);
  });
});

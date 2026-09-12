import { afterEach, describe, expect, it, vi } from "vitest";
import { AGENT_IDS } from "./env";
import {
  BUILTIN_MODELS,
  DEFAULT_AGENT_ID,
  DEFAULT_SETTINGS,
  dedupeModels,
  loadActiveAgent,
  loadConversations,
  loadSettings,
  normalizeConversation,
  saveActiveAgent,
  saveConversations,
  saveSettings,
} from "./storage";

/** 用一份内存实现替换 localStorage，并在用例结束后自动还原。 */
function stubStorage(impl: Pick<Storage, "getItem" | "setItem">): void {
  vi.stubGlobal("localStorage", impl);
}

/** 模拟「存储被禁用」：任何访问都直接抛异常（WebView 隐私模式的真实行为）。 */
function stubBrokenStorage(): void {
  stubStorage({
    getItem() {
      throw new Error("SecurityError: storage is disabled");
    },
    setItem() {
      throw new Error("SecurityError: storage is disabled");
    },
  });
}

/** 模拟一块可用的存储，可预置若干键值。 */
function stubMemoryStorage(initial?: ReadonlyArray<readonly [string, string]>): void {
  const map = new Map<string, string>(initial ?? []);
  stubStorage({
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => {
      map.set(k, v);
    },
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("DEFAULT_AGENT_ID", () => {
  it("is one of the three known agents", () => {
    expect(AGENT_IDS).toContain(DEFAULT_AGENT_ID);
  });
});

describe("normalizeConversation", () => {
  it("fills every field of a legacy record that predates the agent field", () => {
    const conv = normalizeConversation({ id: "a1", title: "旧会话" });
    expect(conv.id).toBe("a1");
    expect(conv.title).toBe("旧会话");
    expect(conv.messages).toEqual([]);
    expect(conv.agent).toBe(DEFAULT_AGENT_ID);
    expect(conv.modelId.length).toBeGreaterThan(0);
    expect(conv.createdAt).toBeGreaterThan(0);
    expect(conv.updatedAt).toBeGreaterThan(0);
  });

  it("keeps an explicit agent instead of overwriting it", () => {
    expect(normalizeConversation({ id: "b1", agent: "codex" }).agent).toBe("codex");
  });

  it("invents a usable id and title when the record is empty", () => {
    const conv = normalizeConversation({});
    expect(conv.id.length).toBeGreaterThan(0);
    expect(conv.title).toBe("新会话");
  });

  it("preserves messages and timestamps verbatim", () => {
    const conv = normalizeConversation({
      id: "c1",
      messages: [{ id: "m1", role: "user", content: "hi", createdAt: 1 }],
      createdAt: 111,
      updatedAt: 222,
    });
    expect(conv.messages).toHaveLength(1);
    expect(conv.createdAt).toBe(111);
    expect(conv.updatedAt).toBe(222);
  });
});

describe("dedupeModels", () => {
  it("keeps the first occurrence of a duplicated id", () => {
    const out = dedupeModels(
      [{ ...BUILTIN_MODELS[0]!, name: "first" }],
      [{ ...BUILTIN_MODELS[0]!, name: "second" }]
    );
    expect(out).toHaveLength(1);
    expect(out[0]!.name).toBe("first");
  });

  it("preserves group order", () => {
    const a = { ...BUILTIN_MODELS[0]!, id: "a" };
    const b = { ...BUILTIN_MODELS[0]!, id: "b" };
    expect(dedupeModels([a], [b]).map((m) => m.id)).toEqual(["a", "b"]);
  });
});

// 下面这组用例针对 storage 的容错读写（readRaw / writeRaw / readJson）：
// 持久化只是锦上添花，存储不可用时界面仍然必须能起来。
describe("storage resilience", () => {
  it("falls back to empty state when localStorage does not exist at all", () => {
    expect(typeof globalThis.localStorage).toBe("undefined");
    expect(loadConversations()).toEqual([]);
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS);
    expect(loadActiveAgent()).toBe(DEFAULT_AGENT_ID);
    // 写操作也不能抛：Node 环境下没有 localStorage，写应当静默跳过。
    expect(() => saveConversations([])).not.toThrow();
    expect(() => saveSettings(DEFAULT_SETTINGS)).not.toThrow();
  });

  it("treats a throwing localStorage as empty instead of crashing", () => {
    stubBrokenStorage();
    expect(loadConversations()).toEqual([]);
    expect(loadSettings()).toEqual(DEFAULT_SETTINGS);
    expect(loadActiveAgent()).toBe(DEFAULT_AGENT_ID);
    expect(() => saveConversations([])).not.toThrow();
    expect(() => saveActiveAgent("codex")).not.toThrow();
  });

  it("round-trips conversations and the active agent through a working storage", () => {
    stubMemoryStorage();
    const conv = normalizeConversation({ id: "r1", title: "往返", agent: "codex" });
    saveConversations([conv]);
    expect(loadConversations()).toEqual([conv]);

    saveActiveAgent("deepharness");
    expect(loadActiveAgent()).toBe("deepharness");
  });

  it("rejects an unknown agent id written by an older or tampered build", () => {
    stubMemoryStorage();
    saveActiveAgent("some-removed-agent");
    expect(loadActiveAgent()).toBe(DEFAULT_AGENT_ID);
  });

  it("survives corrupt JSON in the conversation slot", () => {
    stubMemoryStorage([["dh.conversations", "{not json"]]);
    expect(loadConversations()).toEqual([]);
  });

  it("ignores a stored value that is valid JSON but not an array", () => {
    stubMemoryStorage([["dh.conversations", '{"oops":true}']]);
    expect(loadConversations()).toEqual([]);
  });
});

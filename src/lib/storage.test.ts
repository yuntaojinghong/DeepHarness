import { describe, expect, it } from "vitest";
import { AGENT_IDS } from "./env";
import { DEFAULT_AGENT_ID, dedupeModels, normalizeConversation } from "./storage";
import { BUILTIN_MODELS } from "./storage";

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

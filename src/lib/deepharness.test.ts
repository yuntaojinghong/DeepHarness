import { describe, expect, it } from "vitest";
import { AGENT_IDS } from "./env";
import {
  AGENT_META,
  agentList,
  agentOpenWebUi,
  agentStateLabel,
  agentStateTone,
  deepharnessStatus,
  describeError,
  formatArgs,
  memoryKindLabel,
  MEMORY_KINDS,
  truncateResult,
  type AgentStatusView,
} from "./deepharness";

describe("AGENT_META", () => {
  it("covers exactly the three agent ids", () => {
    expect(Object.keys(AGENT_META).sort()).toEqual([...AGENT_IDS].sort());
  });

  it("marks only deepharness as a task-console agent", () => {
    const taskAgents = AGENT_IDS.filter((id) => AGENT_META[id].hasTaskConsole);
    expect(taskAgents).toEqual(["deepharness"]);
  });

  it("gives the harness agent a loopback base url", () => {
    // 这是**基础地址**，只用于展示。真正打开要走 agentOpenWebUi()：
    // dsh 0.1.5 起每次都要求一个重新生成的一次性 token。
    const url = AGENT_META["deepseek-harness"].webUiUrl;
    expect(url).toBe("http://127.0.0.1:3080");
    expect(url).not.toContain("token");
  });

  it("gives every agent a non-empty name and tagline", () => {
    for (const id of AGENT_IDS) {
      expect(AGENT_META[id].name.length).toBeGreaterThan(0);
      expect(AGENT_META[id].tagline.length).toBeGreaterThan(0);
    }
  });
});

describe("agent state presentation", () => {
  const cases: [AgentStatusView, string, string][] = [
    [{ state: "running" }, "运行中", "ok"],
    [{ state: "starting" }, "启动中", "busy"],
    [{ state: "crashed", reason: "boom" }, "已崩溃", "err"],
    [{ state: "stopped" }, "已停止", ""],
  ];

  it.each(cases)("renders %j", (status, label, tone) => {
    expect(agentStateLabel(status)).toBe(label);
    expect(agentStateTone(status)).toBe(tone);
  });

  it("degrades gracefully when the overview is missing", () => {
    expect(agentStateLabel(undefined)).toBe("未知");
    expect(agentStateTone(undefined)).toBe("");
  });
});

describe("truncateResult", () => {
  it("returns short results untouched", () => {
    expect(truncateResult("ok", 10)).toBe("ok");
  });

  it("marks truncation and reports the original length", () => {
    const long = "x".repeat(25);
    const out = truncateResult(long, 10);
    expect(out.startsWith("x".repeat(10))).toBe(true);
    expect(out).toContain("共 25 字符");
  });
});

describe("formatArgs", () => {
  it("returns an empty string for absent args", () => {
    expect(formatArgs(null)).toBe("");
    expect(formatArgs(undefined)).toBe("");
  });

  it("serialises objects to compact json", () => {
    expect(formatArgs({ path: ".", depth: 2 })).toBe('{"path":".","depth":2}');
  });

  it("does not throw on circular structures", () => {
    const circular: Record<string, unknown> = {};
    circular["self"] = circular;
    expect(() => formatArgs(circular)).not.toThrow();
  });
});

describe("describeError", () => {
  it("unwraps Error instances", () => {
    expect(describeError(new Error("炸了"))).toBe("炸了");
  });

  it("passes plain strings through, as Tauri rejects with strings", () => {
    expect(describeError("拒绝删除：目录非空")).toBe("拒绝删除：目录非空");
  });

  it("stringifies objects instead of printing [object Object]", () => {
    expect(describeError({ code: 7 })).toBe('{"code":7}');
  });

  it("falls back to String for primitives", () => {
    expect(describeError(42)).toBe("42");
    expect(describeError(null)).toBe("null");
  });
});

describe("memory kinds", () => {
  it("mirrors the Rust MEMORY_KINDS list", () => {
    expect([...MEMORY_KINDS]).toEqual(["fact", "preference", "event", "reflection"]);
  });

  it("translates known kinds and falls back to the raw value", () => {
    expect(memoryKindLabel("reflection")).toBe("反思");
    expect(memoryKindLabel("unknown-kind")).toBe("unknown-kind");
  });
});

describe("browser preview fallback", () => {
  // 测试运行在 node 环境，isTauri() 为 false，正好覆盖「浏览器预览」分支。
  it("reports three stopped agents without touching the backend", async () => {
    const list = await agentList();
    expect(list).toHaveLength(3);
    expect(list.map((a) => a.id).sort()).toEqual([...AGENT_IDS].sort());
    expect(list.every((a) => a.status.state === "stopped")).toBe(true);
  });

  it("reports the worker as not running", async () => {
    await expect(deepharnessStatus()).resolves.toEqual({ running: false, configured: false });
  });

  it("refuses to open the official web ui outside the desktop shell", async () => {
    // 打开动作必须由后端完成（token 与弹窗手势都在那一侧处理），
    // 浏览器预览下只能明确拒绝，而不是静默打开一个 401 页面。
    await expect(agentOpenWebUi("deepseek-harness")).rejects.toThrow(/桌面版/);
  });
});

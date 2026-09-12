import { describe, expect, it } from "vitest";
import { AGENT_TOOLS, AGENT_TOOL_NAMES, FORBIDDEN_TOOL_NAMES } from "./agent-tools";

/**
 * 守卫测试：内置工具集与「界面上展示的开关」必须始终一致。
 *
 * 这组断言的由来是真实缺陷 —— 右侧面板曾摆着「代码执行」「网页搜索」两个开关，
 * 但 `AGENT_TOOLS` 里根本没有对应工具，拨动开关不会带来任何能力变化
 * （三个开关实际只影响同一个布尔值）。因此这里把「工具集本身」和
 * 「界面文案来源」都钉死，防止再次跑偏。
 *
 * 读取源码用 Vite 的 `import.meta.glob(?raw)`，与 `selector-guard.test.ts` 一致：
 * 无需 `@types/node`，也不必关心 cwd。
 */
const SOURCES = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as unknown as Record<string, string>;

/** 按路径后缀取源码；取不到直接抛错，避免守卫静默空转。 */
function read(suffix: string): string {
  const key = Object.keys(SOURCES).find((k) => k.endsWith(suffix));
  // tsconfig 开了 noUncheckedIndexedAccess，索引结果类型带 undefined，
  // 因此这里先取到值再判空（而不是只判 key）。
  const source = key ? SOURCES[key] : undefined;
  if (!source) throw new Error(`未找到源码文件：${suffix}`);
  return source;
}

describe("AGENT_TOOLS 结构", () => {
  it("非空且每一项都是合法的函数工具描述", () => {
    expect(AGENT_TOOLS.length).toBeGreaterThan(0);
    for (const t of AGENT_TOOLS) {
      expect(t.type).toBe("function");
      expect(typeof t.function.name).toBe("string");
      expect(t.function.name.length).toBeGreaterThan(0);
      expect(t.function.description.length).toBeGreaterThan(10);
      expect(t.function.parameters.type).toBe("object");
      expect(t.function.parameters.required.length).toBeGreaterThan(0);
    }
  });

  it("工具名唯一", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("每个必填参数都有对应的属性声明（否则模型无法构造调用）", () => {
    for (const t of AGENT_TOOLS) {
      for (const req of t.function.parameters.required) {
        expect(
          Object.prototype.hasOwnProperty.call(t.function.parameters.properties, req),
          `${t.function.name} 的必填参数 ${req} 缺少 properties 声明`,
        ).toBe(true);
      }
    }
  });

  it("AGENT_TOOL_NAMES 与 AGENT_TOOLS 一一对应", () => {
    expect(AGENT_TOOL_NAMES).toEqual(AGENT_TOOLS.map((t) => t.function.name));
  });

  it("当前工具集就是三个文件工具", () => {
    expect([...AGENT_TOOL_NAMES].sort()).toEqual(["list_dir", "read_file", "write_file"]);
  });
});

describe("安全边界：不得出现命令执行 / 网络访问工具", () => {
  it("工具集内没有禁用名字", () => {
    const banned = new Set(FORBIDDEN_TOOL_NAMES.map((n) => n.toLowerCase()));
    for (const t of AGENT_TOOLS) {
      expect(banned.has(t.function.name.toLowerCase()), `${t.function.name} 属于禁用工具`).toBe(false);
    }
  });

  it("禁用名单本身非空且覆盖关键项", () => {
    expect(FORBIDDEN_TOOL_NAMES.length).toBeGreaterThanOrEqual(6);
    expect(FORBIDDEN_TOOL_NAMES).toContain("run_command");
    expect(FORBIDDEN_TOOL_NAMES).toContain("web_search");
  });

  it("ensure：ChatArea 源码里不再出现 run_command", () => {
    expect(read("components/ChatArea.tsx")).not.toContain("run_command");
  });
});

describe("界面文案必须派生自工具集（防止再次跑偏）", () => {
  it("ContextPanel 导入 AGENT_TOOL_NAMES 而不是硬编码工具名", () => {
    expect(read("components/ContextPanel.tsx")).toContain("AGENT_TOOL_NAMES");
  });

  it("ContextPanel 不再出现无对应工具的开关文案", () => {
    const panel = read("components/ContextPanel.tsx");
    for (const stale of ["代码执行", "网页搜索"]) {
      expect(panel, `「${stale}」没有对应工具，不应再出现在面板上`).not.toContain(stale);
    }
  });

  it("ContextPanel 的开关绑定 fileTools（唯一真实开关）", () => {
    const panel = read("components/ContextPanel.tsx");
    expect(panel).toContain("tools.fileTools");
    expect(panel).not.toContain("tools.code");
    expect(panel).not.toContain("tools.search");
  });

  it("ChatArea 从 lib 引入工具集，而不是就地再定义一份", () => {
    const chat = read("components/ChatArea.tsx");
    expect(chat).toContain('from "../lib/agent-tools"');
    // 就地定义会同时出现「const AGENT_TOOLS =」这种赋值
    expect(chat).not.toMatch(/const\s+AGENT_TOOLS\s*=/);
  });

  it("启用条件只依赖 fileTools", () => {
    expect(read("components/ChatArea.tsx")).toContain("model.supportsTools && tools.fileTools");
  });
});

describe("ToolToggle 形状与开关一致", () => {
  it("ToolToggle 只有 fileTools 字段", () => {
    const types = read("types.ts");
    const block = /export interface ToolToggle \{(.*?)\n\}/s.exec(types)?.[1] ?? "";
    expect(block).toContain("fileTools");
    expect(block).not.toMatch(/\bcode\b\s*:/);
    expect(block).not.toMatch(/\bsearch\b\s*:/);
  });

  it("store 默认值与 ToolToggle 形状一致", () => {
    const store = read("store.ts");
    expect(store).toContain("tools: { fileTools: true }");
    expect(store).not.toContain("tools: { code:");
  });
});

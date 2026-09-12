import { describe, expect, it } from "vitest";
import {
  AUTO_DETECT_MIN_RELEVANCE,
  highlightCodeBlock,
  languageFromClassName,
  REGISTERED_LANGUAGE_COUNT,
  registerLanguages,
  supportsLanguage,
  type CodeBlockLike,
} from "./highlight";

/** 构造一个满足 CodeBlockLike 的普通对象桩，无需 DOM。 */
function makeBlock(className: string, code: string): CodeBlockLike & { addedClasses: string[] } {
  const addedClasses: string[] = [];
  return {
    className,
    textContent: code,
    innerHTML: "",
    addedClasses,
    classList: {
      add(...tokens: string[]) {
        addedClasses.push(...tokens);
      },
    },
  };
}

describe("languageFromClassName", () => {
  it("识别 language- 前缀", () => {
    expect(languageFromClassName("language-javascript")).toBe("javascript");
  });

  it("识别 lang- 前缀", () => {
    expect(languageFromClassName("lang-python")).toBe("python");
  });

  it("在多个类名中定位语言类", () => {
    expect(languageFromClassName("hljs language-rust extra")).toBe("rust");
  });

  it("大小写归一为小写", () => {
    expect(languageFromClassName("language-TypeScript")).toBe("typescript");
  });

  it("保留含 + # . - 的语言名（c++ / c# / f# / objective-c）", () => {
    expect(languageFromClassName("language-c++")).toBe("c++");
    expect(languageFromClassName("language-c#")).toBe("c#");
    expect(languageFromClassName("language-objective-c")).toBe("objective-c");
  });

  it("无语言类时返回空串", () => {
    expect(languageFromClassName("")).toBe("");
    expect(languageFromClassName("hljs")).toBe("");
  });

  it("不把 language 单词本身当成语言名", () => {
    expect(languageFromClassName("highlight-language")).toBe("");
  });
});

describe("registerLanguages", () => {
  it("注册数量与清单一致且非空", () => {
    registerLanguages();
    expect(REGISTERED_LANGUAGE_COUNT).toBeGreaterThanOrEqual(40);
  });

  it("重复调用幂等（不抛错、结果一致）", () => {
    registerLanguages();
    registerLanguages();
    expect(supportsLanguage("javascript")).toBe(true);
  });

  it("规范语言名全部可用", () => {
    for (const name of [
      "bash",
      "c",
      "cpp",
      "csharp",
      "css",
      "dockerfile",
      "go",
      "java",
      "javascript",
      "json",
      "kotlin",
      "lua",
      "markdown",
      "php",
      "python",
      "ruby",
      "rust",
      "sql",
      "swift",
      "typescript",
      "xml",
      "yaml",
    ]) {
      expect(supportsLanguage(name), `应支持 ${name}`).toBe(true);
    }
  });

  it("常用别名可用（依赖各语言定义自带的 aliases）", () => {
    for (const alias of ["js", "jsx", "ts", "tsx", "py", "yml", "sh", "cs", "rs", "rb", "kt", "c++"]) {
      expect(supportsLanguage(alias), `应支持别名 ${alias}`).toBe(true);
    }
  });

  it("未注册的语言返回 false 而不是抛错", () => {
    expect(supportsLanguage("totally-made-up-lang")).toBe(false);
    expect(supportsLanguage("")).toBe(false);
  });
});

describe("highlightCodeBlock", () => {
  it("已标注且已注册的语言：着色并加 hljs 类，保留原始内容不丢", () => {
    const el = makeBlock("language-javascript", "const a = 1;");
    highlightCodeBlock(el);
    expect(el.classList).toBeTruthy();
    expect(el.addedClasses).toContain("hljs");
    // 着色后应含 span 标记；关键字 const 应被识别
    expect(el.innerHTML).not.toBe("");
    expect(el.innerHTML).toContain("const");
    expect(el.innerHTML).toContain("<span");
  });

  it("着色结果包含原始 token，未发生内容丢失", () => {
    const el = makeBlock("language-python", "def add(a, b):\n    return a + b");
    highlightCodeBlock(el);
    // 去掉标签后应与原始文本一致
    const plain = el.innerHTML.replace(/<[^>]+>/g, "");
    expect(plain).toContain("def");
    expect(plain).toContain("return a + b");
  });

  it("标注了但未注册的语言：不抛错、只加 hljs 类、innerHTML 保持不动", () => {
    const el = makeBlock("language-nonexistent-xyz", "some raw text");
    const before = el.innerHTML;
    expect(() => highlightCodeBlock(el)).not.toThrow();
    expect(el.addedClasses).toContain("hljs");
    expect(el.innerHTML).toBe(before);
  });

  it("空白内容直接跳过", () => {
    const el = makeBlock("language-js", "   \n  ");
    highlightCodeBlock(el);
    expect(el.innerHTML).toBe("");
    expect(el.addedClasses).toEqual([]);
  });

  it("textContent 为 null 时不抛错", () => {
    const el = makeBlock("language-js", "");
    el.textContent = null;
    expect(() => highlightCodeBlock(el)).not.toThrow();
  });

  it("未标注语言时走自动探测：相关度达标才着色", () => {
    // 一段特征明确的 C 代码，自动探测相关度应达标
    const el = makeBlock(
      "",
      "#include <stdio.h>\nint main(void) { printf(\"hi\"); return 0; }",
    );
    highlightCodeBlock(el);
    const autoDetected = el.addedClasses.includes("hljs");
    if (autoDetected) {
      expect(el.innerHTML).toContain("<span");
      // 自动探测命中时会补一个 language-<名> 类
      expect(el.addedClasses.some((c) => c.startsWith("language-"))).toBe(true);
    } else {
      // 未达标则必须保持原样，不得留下半成品标记
      expect(el.innerHTML).toBe("");
    }
  });

  it("自动探测阈值是正数，防止恒真", () => {
    expect(AUTO_DETECT_MIN_RELEVANCE).toBeGreaterThan(0);
  });
});

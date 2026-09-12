/**
 * highlight.js 按需注册与代码块着色。
 *
 * ## 为什么不用 `import hljs from "highlight.js"`
 * 那个入口会把 **386 份**语言定义一次性打进产物（压缩后约 1 MB）。而 AI 对话里
 * 出现的代码块语言高度集中，因此这里改用 `highlight.js/lib/core` + 显式注册
 * 48 种常用语言：实测高亮模块从 1014 kB 降到约 150 kB。
 *
 * ## 未注册语言的处理（不允许抛错或丢内容）
 * - 标注了语言但未注册 → 保持纯文本，**不进 hljs**（否则 hljs 会打警告）
 * - 未标注语言 → 交给自动探测，相关度不足时不着色（避免把普通文本染花）
 * - 单块着色失败 → 只影响该块观感，其余块照常
 *
 * ## 测试性
 * 核心逻辑 `highlightCodeBlock` 只依赖一个最小结构接口，不依赖真实 DOM，
 * 因此可直接用普通对象桩在 node 环境下断言（见 highlight.test.ts）。
 */
import hljs from "highlight.js/lib/core";
import type { LanguageFn } from "highlight.js";

import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dart from "highlight.js/lib/languages/dart";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import elixir from "highlight.js/lib/languages/elixir";
import erlang from "highlight.js/lib/languages/erlang";
import go from "highlight.js/lib/languages/go";
import graphql from "highlight.js/lib/languages/graphql";
import groovy from "highlight.js/lib/languages/groovy";
import haskell from "highlight.js/lib/languages/haskell";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import julia from "highlight.js/lib/languages/julia";
import kotlin from "highlight.js/lib/languages/kotlin";
import latex from "highlight.js/lib/languages/latex";
import less from "highlight.js/lib/languages/less";
import lua from "highlight.js/lib/languages/lua";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import matlab from "highlight.js/lib/languages/matlab";
import nginx from "highlight.js/lib/languages/nginx";
import nim from "highlight.js/lib/languages/nim";
import nix from "highlight.js/lib/languages/nix";
import objectivec from "highlight.js/lib/languages/objectivec";
import perl from "highlight.js/lib/languages/perl";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import rLang from "highlight.js/lib/languages/r";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scala from "highlight.js/lib/languages/scala";
import scss from "highlight.js/lib/languages/scss";
import shell from "highlight.js/lib/languages/shell";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import vbnet from "highlight.js/lib/languages/vbnet";
import vim from "highlight.js/lib/languages/vim";
import wasm from "highlight.js/lib/languages/wasm";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

/**
 * 规范语言名 → 语言定义。
 * 名称必须是 hljs 的**规范名**（`hljs.listLanguages()` 里的那种）；
 * 常见别名由各语言定义自带的 `aliases` 提供，例如
 * javascript → js/jsx/mjs、typescript → ts/tsx、python → py/pyw、
 * yaml → yml、bash → sh、csharp → cs、cpp → c++/cc/hpp、rust → rs。
 */
const LANGUAGES: ReadonlyArray<readonly [string, LanguageFn]> = [
  ["bash", bash],
  ["c", c],
  ["cpp", cpp],
  ["csharp", csharp],
  ["css", css],
  ["dart", dart],
  ["diff", diff],
  ["dockerfile", dockerfile],
  ["elixir", elixir],
  ["erlang", erlang],
  ["go", go],
  ["graphql", graphql],
  ["groovy", groovy],
  ["haskell", haskell],
  ["ini", ini],
  ["java", java],
  ["javascript", javascript],
  ["json", json],
  ["julia", julia],
  ["kotlin", kotlin],
  ["latex", latex],
  ["less", less],
  ["lua", lua],
  ["makefile", makefile],
  ["markdown", markdown],
  ["matlab", matlab],
  ["nginx", nginx],
  ["nim", nim],
  ["nix", nix],
  ["objectivec", objectivec],
  ["perl", perl],
  ["php", php],
  ["powershell", powershell],
  ["python", python],
  ["r", rLang],
  ["ruby", ruby],
  ["rust", rust],
  ["scala", scala],
  ["scss", scss],
  ["shell", shell],
  ["sql", sql],
  ["swift", swift],
  ["typescript", typescript],
  ["vbnet", vbnet],
  ["vim", vim],
  ["wasm", wasm],
  ["xml", xml],
  ["yaml", yaml],
];

/** 已注册的语言数量，用于测试断言与诊断输出。 */
export const REGISTERED_LANGUAGE_COUNT = LANGUAGES.length;

/** 自动探测的最低相关度：低于此值不着色，避免普通中文/英文被误染。 */
export const AUTO_DETECT_MIN_RELEVANCE = 5;

let registered = false;

/**
 * 幂等注册全部内置语言。可在任意处安全重复调用。
 * hljs 的 `registerLanguage` 对同名重复注册会覆盖而非报错，
 * 但仍加一次性守卫，省掉不必要的重复编译。
 */
export function registerLanguages(): void {
  if (registered) return;
  for (const [name, definition] of LANGUAGES) {
    hljs.registerLanguage(name, definition);
  }
  registered = true;
}

/** 该语言名（含别名）是否可用。便于测试与降级判断。 */
export function supportsLanguage(name: string): boolean {
  registerLanguages();
  return Boolean(hljs.getLanguage(name));
}

/** 从 `class="language-xxx"` / `class="lang-xxx"` 中取出小写语言名；取不到返回空串。 */
export function languageFromClassName(className: string): string {
  const matched = /(?:^|\s)(?:language|lang)-([\w+#.-]+)/i.exec(className || "");
  // tsconfig 开了 noUncheckedIndexedAccess，matched[1] 的类型是 string | undefined，
  // 即便 matched 非空也不能直接 .toLowerCase()。
  const name = matched?.[1];
  return name ? name.toLowerCase() : "";
}

/**
 * 着色所需的最小元素结构。
 * 刻意不写成 `HTMLElement`，以便用普通对象桩测试（node 环境无 DOM）。
 * 真实 `HTMLElement` 结构上即满足此接口。
 */
export interface CodeBlockLike {
  className: string;
  textContent: string | null;
  innerHTML: string;
  classList: { add(...tokens: string[]): void };
}

/**
 * 给单个代码块着色。就地修改 `el`，不返回内容。
 * 无语言标注且自动探测相关度不足时保持原样。
 */
export function highlightCodeBlock(el: CodeBlockLike): void {
  registerLanguages();

  const source = el.textContent ?? "";
  if (!source.trim()) return;

  const requested = languageFromClassName(el.className);

  if (requested) {
    // 标注了但未注册：加 hljs 类走主题基础样式，内容保持纯文本。
    if (!hljs.getLanguage(requested)) {
      el.classList.add("hljs");
      return;
    }
    el.innerHTML = hljs.highlight(source, { language: requested, ignoreIllegals: true }).value;
    el.classList.add("hljs");
    return;
  }

  const auto = hljs.highlightAuto(source);
  if (auto.language && auto.relevance >= AUTO_DETECT_MIN_RELEVANCE) {
    el.innerHTML = auto.value;
    el.classList.add("hljs", `language-${auto.language}`);
  }
}

/**
 * 给容器内所有 `pre code` 着色。
 * 逐块隔离异常：某一块失败不影响其余块，也不冒泡打断渲染。
 */
export function highlightCodeBlocks(root: ParentNode): void {
  registerLanguages();
  for (const el of root.querySelectorAll<HTMLElement>("pre code")) {
    try {
      highlightCodeBlock(el);
    } catch {
      /* 着色属于观感增强，失败时保留 marked 输出的纯文本即可 */
    }
  }
}

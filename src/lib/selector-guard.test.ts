import { describe, expect, it } from "vitest";

/**
 * 静态守卫：**禁止在选择器里调用 store 方法**。
 *
 * 背景（真实事故，2026-09-13）：`Sidebar` 曾写
 * `useAppStore((s) => s.searchConversations())`，而该方法内部是
 * `list.filter(...)`——每次调用都返回新数组。zustand v5 的 `useStore` 用
 * `Object.is` 比较选择器返回值，快照恒不相等导致 React 无限重渲染，
 * 最终抛 #185「Maximum update depth exceeded」并卸载整棵组件树。
 * 表现是启动动画播完就只剩一片深色底（黑屏），且生产构建里连一句
 * 警告都不会留下——排查成本极高。
 *
 * 规则：选择器只允许取状态切片（`s.someField`），派生计算放进 `useMemo`
 * 或 `src/lib/hooks.ts` 的现成钩子里。
 */

const SOURCES = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as unknown as Record<string, string>;

/**
 * `useAppStore((s) => s.someMethod(` —— 选择器体内对 **store 实例本身** 发起调用。
 *
 * 用反向引用把「参数名」和「点号左边的接收者」绑成同一个标识符，
 * 这样 `useAppStore((s) => Object.values(s.x).some(...))` 这类
 * 「以 store 之外的东西做接收者」的写法不会被误判（App.tsx 就是这种，
 * 它返回的是布尔值，快照稳定，是安全的）。
 */
const OFFENDING =
  /useAppStore\(\s*\(?\s*([A-Za-z_$][\w$]*)\s*\)?\s*=>\s*\1\s*\.\s*[A-Za-z_$][\w$]*\s*\(/;

function isTestFile(path: string): boolean {
  return /\.test\.tsx?$/.test(path);
}

/** 统一斜杠方向并去掉 `./` `../` 前缀，便于按文件名定位。 */
function normalize(path: string): string {
  return path.replace(/\\/g, "/").replace(/^(\.\.?\/)+/, "");
}

/**
 * 按**文件名**定位源码。
 *
 * 不用完整相对路径：Vite 的 `import.meta.glob` 返回的键在处理 `../` 前缀时
 * 会丢掉一层目录（例如 `src/lib/hooks.ts` 的键是 `../hooks.ts`），
 * 按 basename 匹配对键格式免疫。下面的文件总数断言负责保证覆盖没有缩水。
 */
function sourceByBasename(name: string): string | undefined {
  return Object.entries(SOURCES).find(([path]) => normalize(path).split("/").pop() === name)?.[1];
}

describe("store 选择器稳定性守卫", () => {
  it("raw 源码加载生效（否则守卫会静默空转）", () => {
    const keys = Object.keys(SOURCES).map(normalize).sort();

    // 只统计非测试文件，防止将来把测试挪走后守卫覆盖面积悄悄变小。
    const scanned = keys.filter((k) => !isTestFile(k));
    expect(scanned.length, `扫描到的文件：${keys.join(", ")}`).toBeGreaterThanOrEqual(30);

    expect(sourceByBasename("store.ts"), `没扫描到 store.ts；实际文件：${keys.join(", ")}`).toContain(
      "create<AppState>",
    );
    expect(sourceByBasename("hooks.ts"), `没扫描到 hooks.ts；实际文件：${keys.join(", ")}`).toContain(
      "useActiveConversation",
    );
  });

  it("没有任何源码在选择器里调用 store 方法", () => {
    const offenders: string[] = [];

    for (const [path, code] of Object.entries(SOURCES)) {
      if (isTestFile(path)) continue; // 守卫测试自身会写出反例
      code.split("\n").forEach((line, index) => {
        // 行内注释里的示例不参与判定，避免文档把自己绊倒。
        const withoutComment = line.replace(/\/\/.*$/, "");
        if (OFFENDING.test(withoutComment)) {
          offenders.push(`${path}:${index + 1}  ${line.trim()}`);
        }
      });
    }

    expect(
      offenders,
      [
        "选择器里不允许调用 store 方法：它可能返回新引用，导致 React 无限重渲染（#185）后整棵树被卸载。",
        "请改成「选择器只取状态切片 + useMemo 派生」；通用场景直接用 src/lib/hooks.ts 里的钩子：",
        ...offenders.map((o) => `  - ${o}`),
      ].join("\n"),
    ).toEqual([]);
  });
});

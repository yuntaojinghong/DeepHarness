import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AUTHOR_NAME,
  ISSUES_URL,
  REPO_URL,
  SUPPORT_QR_CODE,
  openExternalUrl,
  openExternalUrlSafe,
} from "./links";

/**
 * 后端（`src-tauri/src/open_url.rs`）的 `ALLOWED_WEB_HOSTS` 白名单副本。
 *
 * 两处必须保持一致：前端拼出的链接一旦落在白名单之外，后端会直接拒绝，
 * 表现为「按钮点了没反应 + 一条报错」。TS 测试跑不到 Rust 侧，
 * 只能用这份副本把契约固化下来 —— 改后端白名单时这里也要改。
 */
const BACKEND_ALLOWED_WEB_HOSTS = ["github.com", "platform.deepseek.com"];

function hostOf(url: string): string {
  return new URL(url).host;
}

function isAllowedByBackend(url: string): boolean {
  if (!url.startsWith("https://")) return false;
  const host = hostOf(url);
  return BACKEND_ALLOWED_WEB_HOSTS.some(
    (allowed) => host === allowed || host.endsWith(`.${allowed}`),
  );
}

describe("项目链接常量", () => {
  it("uses https for the repository and issues urls", () => {
    expect(REPO_URL.startsWith("https://")).toBe(true);
    expect(ISSUES_URL.startsWith("https://")).toBe(true);
  });

  it("keeps every link inside the backend whitelist", () => {
    // 这是跨层契约：链接域名改了但没同步后端白名单，功能会静默失效。
    for (const url of [REPO_URL, ISSUES_URL]) {
      expect(isAllowedByBackend(url), `${url} 不在后端白名单内`).toBe(true);
    }
  });

  it("points ISSUES_URL at the repository's issues page", () => {
    expect(ISSUES_URL.startsWith(REPO_URL)).toBe(true);
    expect(ISSUES_URL.endsWith("/issues")).toBe(true);
  });

  it("avoids shell metacharacters in the urls it ships", () => {
    // 后端只接受受限字符集（`& | ^` 等会被 cmd 重新解析），
    // 因此常量里不允许出现查询串之类的字符。
    for (const url of [REPO_URL, ISSUES_URL]) {
      expect(url).not.toMatch(/[&|^<>"'`\\\s]/);
    }
  });

  it("exposes an author name for the credit line", () => {
    expect(AUTHOR_NAME.trim().length).toBeGreaterThan(0);
  });

  it("resolves the support qr code to a usable asset url", () => {
    // 构建期由 Vite 处理为资源 URL；这里只要求它是个非空字符串，
    // 不锁死具体形态（dev 下是 /src/assets/...，构建后是带 hash 的名字）。
    expect(typeof SUPPORT_QR_CODE).toBe("string");
    expect(SUPPORT_QR_CODE.length).toBeGreaterThan(0);
    expect(SUPPORT_QR_CODE).toMatch(/support-qrcode/);
  });
});

describe("openExternalUrl", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("falls back to window.open outside the desktop shell", async () => {
    // 测试跑在 node 环境，isTauri() 为 false → 走浏览器分支
    const open = vi.fn();
    vi.stubGlobal("window", { open });

    await openExternalUrl(REPO_URL);

    expect(open).toHaveBeenCalledWith(REPO_URL, "_blank", "noopener,noreferrer");
  });

  it("returns null from the safe wrapper when opening succeeds", async () => {
    const open = vi.fn();
    vi.stubGlobal("window", { open });

    await expect(openExternalUrlSafe(REPO_URL)).resolves.toBeNull();
  });

  it("reports the failure message instead of throwing", async () => {
    // 后端拒绝时 Tauri 以字符串 reject，包装函数必须把它转成返回值，
    // 否则异常会冒到渲染层把弹窗整块打掉。
    vi.stubGlobal("window", {
      open: () => {
        throw "拒绝打开非回环或含非法字符的地址";
      },
    });

    await expect(openExternalUrlSafe("https://evil.com/")).resolves.toBe(
      "拒绝打开非回环或含非法字符的地址",
    );
  });
});

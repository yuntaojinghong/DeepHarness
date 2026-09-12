/**
 * 项目对外信息与「在系统浏览器中打开链接」的统一入口。
 *
 * 与 `deepharness.ts`（Agent IPC 契约）分开：这里放的是应用自身的
 * 元信息与系统级能力，与三个 Agent 无关。
 */

import supportQrCode from "../assets/support-qrcode.png";
import { isTauri } from "./env";

/** 项目仓库主页。 */
export const REPO_URL = "https://github.com/yuntaojinghong/DeepHarness";

/** 问题反馈入口。 */
export const ISSUES_URL = `${REPO_URL}/issues`;

/** 作者署名（赞赏码落款，与仓库 owner 一致）。 */
export const AUTHOR_NAME = "云涛惊鸿";

/** 赞赏码图片（构建期由 Vite 处理为可引用的资源 URL）。 */
export const SUPPORT_QR_CODE = supportQrCode;

/**
 * 在系统默认浏览器中打开链接。
 *
 * 桌面版必须走后端：Tauri WebView 里 `<a target="_blank">` 与
 * `window.open` 都可能被 WebView 自己接管，把应用界面替换掉或直接无效；
 * 后端统一用系统 `start` 打开，才保证一定落到用户的默认浏览器。
 * 后端另有 URL 白名单（回环地址 + 少量公网站点），非法地址会报错。
 *
 * 浏览器预览下退回 `window.open` —— 那种环境里本来就是浏览器，行为正确。
 */
export async function openExternalUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_external_url", { url });
}

/** 打开一个链接，并把失败信息交给调用方展示（不抛到渲染层）。 */
export async function openExternalUrlSafe(url: string): Promise<string | null> {
  try {
    await openExternalUrl(url);
    return null;
  } catch (e) {
    return typeof e === "string" ? e : e instanceof Error ? e.message : "打开链接失败";
  }
}

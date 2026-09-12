/**
 * DeepHarness 前端 E2E 验证（无浏览器自动化工具时的替代方案）
 *
 * 三段式，全部在本进程内编排，每一步都有独立超时，绝不挂死：
 *   1) Node `createServer` 托管 `dist/`，并 fetch 自检（HTML / 入口 JS / 异步块可达）
 *   2) `spawn` 无头 Edge（`--headless=new --remote-debugging-port`）
 *   3) CDP：只选**真正的 page target** → 先导航 → 写入 localStorage 种子数据
 *      → reload → 收异常/日志/DOM 断言/网络请求/截图
 *
 * 为什么要「先导航再写 localStorage 再 reload」：localStorage 按源隔离，
 * 必须在目标源上才能写；写完后必须 reload 才会被 store 初始化读取。
 *
 * 断言覆盖本轮改动的高风险点：
 *   - React `lazy` + `Suspense` 是否真的不抛异常（exceptions 必须为 0）
 *   - 代码高亮是否真的生效（`.hljs` / `.hljs-keyword` 必须存在）
 *   - 着色是否**未丢内容**（去掉标签后须包含原始 token）
 *   - 未注册语言是否只是不高亮、不报错（控制台无 hljs 警告）
 *   - markdown 是否确实来自**异步块**（Network 里能看到该 chunk）
 *
 * 踩坑记录（务必保留）：
 *   - CDP `/json/list` 的第一个 target 可能是扩展后台页，必须只认
 *     `type === "page"` 且 URL 非 `chrome-extension://`、非 `_generated_background_page`，
 *     否则 `Page.captureScreenshot` 会挂死、脚本看起来「跑了但没输出」。
 *   - 每条 CDP 命令都要自带超时，否则一条挂住会导致报告永不落盘。
 *   - 报告落盘放在 `finally` 里，失败也要留下 `fatal` 字段可查。
 *
 * 用法：
 *   node scripts/verify-ui.mjs                     # 默认 dist/ 与 Edge 安装路径
 *   DIST=... EDGE=... OUTDIR=... node scripts/verify-ui.mjs
 * 退出码：全部断言通过 0，否则 1。
 */
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { extname, join, normalize, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");

const OUTDIR = process.env.OUTDIR ?? join(ROOT, ".verify-ui");
const DIST = process.env.DIST ?? join(ROOT, "dist");
const EDGE =
  process.env.EDGE ?? "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const HTTP_PORT = Number(process.env.HTTP_PORT ?? 4191);
const CDP_PORT = Number(process.env.CDP_PORT ?? 9241);
const PAGE_URL = `http://127.0.0.1:${HTTP_PORT}/`;
const OUT = join(OUTDIR, "report.json");
const SHOT = join(OUTDIR, "shot.png");

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".json": "application/json; charset=utf-8",
  ".woff2": "font/woff2",
};

/** 种子里刻意混入：已注册语言、未注册语言、无语言标注、行内代码。 */
const SEED_CODE_JS = 'const greeting = "hello";\nfunction add(a, b) {\n  return a + b;\n}';
const SEED_CODE_UNKNOWN = "this is a made up language body 1234";

const SEED = {
  "dh.conversations": JSON.stringify([
    {
      id: "e2e-conv-1",
      title: "E2E 渲染验证",
      agent: "deepseek-harness",
      modelId: "deepseek-v4-flash",
      createdAt: 1700000000000,
      updatedAt: 1700000000000,
      messages: [
        { id: "m1", role: "user", content: "给我一段 JS 示例", createdAt: 1700000000000 },
        {
          id: "m2",
          role: "assistant",
          content:
            "这是一个函数：\n\n```javascript\n" +
            SEED_CODE_JS +
            "\n```\n\n未注册语言的代码块：\n\n```madeup-lang\n" +
            SEED_CODE_UNKNOWN +
            "\n```\n\n无语言标注的代码块：\n\n```\nplain block no language\n```\n\n行内代码 `inlineCode()` 应保留。",
          model: "deepseek-v4-flash",
          createdAt: 1700000000001,
        },
      ],
    },
  ]),
  "dh.activeAgent": "deepseek-harness",
};

const report = {
  env: { OUTDIR, DIST, EDGE, PAGE_URL },
  steps: [],
  httpSelfCheck: null,
  candidates: [],
  target: null,
  exceptions: [],
  console: [],
  logs: [],
  requests: [],
  dom: null,
  assertions: [],
  passed: false,
  screenshotBytes: 0,
  shotError: null,
  fatal: null,
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (s) => {
  report.steps.push(s);
  console.log("[step]", s);
};

let server;
let edge;
let ws;
let msgId = 0;
const pending = new Map();

function send(method, params = {}, timeoutMs = 15000) {
  const id = ++msgId;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`cdp timeout: ${method}`));
    }, timeoutMs);
    pending.set(id, (v) => {
      clearTimeout(timer);
      resolve(v);
    });
    try {
      ws.send(JSON.stringify({ id, method, params }));
    } catch (e) {
      clearTimeout(timer);
      pending.delete(id);
      reject(e);
    }
  });
}

async function cdpList() {
  const res = await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`);
  return await res.json();
}

function isRealPage(t) {
  if (!t || !t.webSocketDebuggerUrl) return false;
  if (t.type !== "page") return false;
  const u = String(t.url ?? "");
  if (u.startsWith("chrome-extension://")) return false;
  if (u.includes("_generated_background_page")) return false;
  return true;
}

/** 断言助手：记录到 report，失败不抛（最后统一判定）。 */
function check(name, ok, detail) {
  report.assertions.push({ name, ok: Boolean(ok), detail: detail ?? null });
  console.log(`[assert] ${ok ? "PASS" : "FAIL"} ${name}${detail ? ` — ${detail}` : ""}`);
  return Boolean(ok);
}

async function main() {
  await rm(OUTDIR, { recursive: true, force: true });
  await mkdir(OUTDIR, { recursive: true });
  step(`outdir ready ${OUTDIR}`);

  // ── 1) 静态服务 ────────────────────────────────────────────────────────
  server = createServer((req, res) => {
    const raw = decodeURIComponent((req.url ?? "/").split("?")[0] ?? "/");
    const rel = normalize(raw).replace(/^(\.\.[/\\])+/, "").replace(/^[/\\]+/, "");
    const file = join(DIST, rel === "" ? "index.html" : rel);
    const ok = (buf, type) => {
      res.writeHead(200, { "content-type": type });
      res.end(buf);
    };
    readFile(file)
      .then((buf) => ok(buf, TYPES[extname(file)] ?? "application/octet-stream"))
      .catch(() =>
        readFile(join(DIST, "index.html"))
          .then((buf) => ok(buf, TYPES[".html"]))
          .catch(() => {
            res.writeHead(404);
            res.end("nf");
          }),
      );
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(HTTP_PORT, "127.0.0.1", resolve);
  });
  step(`static server listening ${HTTP_PORT}`);

  // 自检：入口 JS 可达 + 静态 HTML 里引用的异步块也可达
  try {
    const res = await fetch(PAGE_URL);
    const html = await res.text();
    const mainJs = (html.match(/assets\/index-[A-Za-z0-9_-]+\.js/) ?? [])[0] ?? "";
    const jsRes = mainJs ? await fetch(new URL(mainJs, PAGE_URL)) : null;
    report.httpSelfCheck = {
      indexStatus: res.status,
      htmlLen: html.length,
      mainJs,
      jsStatus: jsRes?.status ?? null,
      jsLen: jsRes ? (await jsRes.text()).length : 0,
    };
  } catch (e) {
    report.httpSelfCheck = { error: String(e) };
  }
  step(`self check: ${JSON.stringify(report.httpSelfCheck)}`);

  // ── 2) 无头 Edge ──────────────────────────────────────────────────────
  const udf = join(OUTDIR, "edge-udf");
  edge = spawn(
    EDGE,
    [
      "--headless=new",
      `--remote-debugging-port=${CDP_PORT}`,
      `--user-data-dir=${udf}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--disable-component-extensions-with-background-pages",
      "--disable-gpu",
      "--disable-logging",
      "--window-size=1440,900",
      "about:blank",
    ],
    { stdio: "ignore", detached: false },
  );
  step(`edge spawned pid=${edge.pid}`);

  // ── 3) 轮询真正的 page target ─────────────────────────────────────────
  let target = null;
  const deadline = Date.now() + 40000;
  let lastErr = "n/a";
  while (Date.now() < deadline) {
    try {
      const list = await cdpList();
      report.candidates = list.map((t) => ({ type: t.type, url: String(t.url).slice(0, 120) }));
      target = list.find(isRealPage) ?? null;
      if (target) break;
      lastErr = `no real page yet: ${JSON.stringify(report.candidates)}`;
    } catch (e) {
      lastErr = String(e?.cause ?? e);
    }
    await sleep(400);
  }
  if (!target) {
    try {
      const res = await fetch(
        `http://127.0.0.1:${CDP_PORT}/json/new?${encodeURIComponent(PAGE_URL)}`,
        { method: "PUT" },
      );
      const t = await res.json();
      if (isRealPage(t)) target = t;
    } catch (e) {
      lastErr += ` | /json/new failed: ${e}`;
    }
  }
  if (!target) throw new Error(`no debuggable page target: ${lastErr}`);
  report.target = { type: target.type, url: target.url, title: target.title };
  step(`target: ${target.type} ${target.url}`);

  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("ws open timeout")), 10000);
    ws.addEventListener(
      "open",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
    ws.addEventListener(
      "error",
      (e) => {
        clearTimeout(timer);
        reject(new Error(`ws: ${e?.message ?? e}`));
      },
      { once: true },
    );
  });

  ws.addEventListener("message", (ev) => {
    let m;
    try {
      m = JSON.parse(typeof ev.data === "string" ? ev.data : String(ev.data));
    } catch {
      return;
    }
    if (m.id && pending.has(m.id)) {
      pending.get(m.id)(m.result ?? m.error ?? {});
      pending.delete(m.id);
      return;
    }
    if (m.method === "Runtime.exceptionThrown") {
      const d = m.params?.exceptionDetails ?? {};
      report.exceptions.push({
        text: d.text,
        description: String(d.exception?.description ?? d.exception?.value ?? "").slice(0, 1200),
      });
    } else if (m.method === "Runtime.consoleAPICalled") {
      const text = (m.params?.args ?? [])
        .map((a) => a.description ?? a.value ?? a.type)
        .join(" ")
        .slice(0, 600);
      report.console.push({ type: m.params?.type, text });
    } else if (m.method === "Log.entryAdded") {
      const e = m.params?.entry ?? {};
      if (!String(e.text ?? "").includes("favicon")) {
        report.logs.push({ level: e.level, text: String(e.text ?? "").slice(0, 500) });
      }
    } else if (m.method === "Network.responseReceived") {
      const u = String(m.params?.response?.url ?? "");
      if (u.includes("/assets/")) {
        report.requests.push({
          url: u.replace(PAGE_URL, ""),
          status: m.params?.response?.status,
          type: m.params?.type,
        });
      }
    }
  });

  await send("Runtime.enable");
  await send("Log.enable");
  await send("Page.enable");
  await send("Network.enable");
  step("domains enabled");

  // ── 4) 首次导航（建立源，才能写 localStorage） ─────────────────────────
  await send("Page.navigate", { url: PAGE_URL });
  await sleep(6000);
  step("navigated (origin established)");

  // ── 5) 写入种子会话 ────────────────────────────────────────────────────
  const seedExpr = `(() => {
    const seed = ${JSON.stringify(SEED)};
    for (const [k, v] of Object.entries(seed)) localStorage.setItem(k, v);
    return JSON.stringify({
      origin: location.origin,
      keys: Object.keys(seed),
      convRawLen: localStorage.getItem("dh.conversations")?.length ?? -1,
    });
  })()`;
  const seedRes = await send("Runtime.evaluate", { expression: seedExpr, returnByValue: true });
  step(`seeded: ${seedRes?.result?.value ?? JSON.stringify(seedRes).slice(0, 300)}`);

  // 从此刻起只关心 reload 之后的异常
  report.exceptions.length = 0;
  report.console.length = 0;
  report.logs.length = 0;
  report.requests.length = 0;

  // ── 6) reload，让 store 初始化时读到种子 ───────────────────────────────
  await send("Page.reload", { ignoreCache: true });
  await sleep(14000);
  step("reloaded with seed");

  // ── 7) DOM 断言 ────────────────────────────────────────────────────────
  const expr = `(() => {
    const root = document.getElementById("root");
    const shell = document.querySelector(".app-shell");
    const t = root ? (root.innerText || "") : "";
    const blocks = [...document.querySelectorAll("pre code")];
    const hl = [...document.querySelectorAll("pre code.hljs")];
    const jsBlock = document.querySelector("pre code.language-javascript");
    const unknownBlock = document.querySelector("pre code.language-madeup-lang");
    const plainBlock = blocks.find(b => !/language-/.test(b.className) && !/^\\s*(const|function)/.test(b.textContent||""));
    const stripTags = (s) => (s || "").replace(/<[^>]+>/g, "");
    const cs = (el) => (el ? getComputedStyle(el) : null);
    const bodyCs = cs(document.body);
    return JSON.stringify({
      href: location.href,
      readyState: document.readyState,
      rootChildCount: root ? root.children.length : -1,
      totalElements: document.querySelectorAll("*").length,
      hasShell: !!shell,
      bodyBg: bodyCs ? bodyCs.backgroundColor : null,
      bodyOpacity: bodyCs ? bodyCs.opacity : null,
      htmlBg: cs(document.documentElement) ? cs(document.documentElement).backgroundColor : null,
      errorCardShown: t.includes("界面渲染出错"),
      triple: {
        deepseekHarness: t.includes("DeepSeek Harness"),
        codex: t.toLowerCase().includes("codex"),
        deepharness: t.includes("DeepHarness"),
      },
      // ── 高亮相关 ──
      codeBlockCount: blocks.length,
      hljsBlockCount: hl.length,
      hljsSpanCount: document.querySelectorAll("pre code span[class^='hljs-']").length,
      keywordSpanCount: document.querySelectorAll("pre code span.hljs-keyword").length,
      jsBlockFound: !!jsBlock,
      jsBlockText: jsBlock ? jsBlock.textContent : null,
      jsBlockHtmlLen: jsBlock ? jsBlock.innerHTML.length : -1,
      jsBlockHasSpan: jsBlock ? jsBlock.innerHTML.includes("<span") : false,
      jsBlockStrippedText: jsBlock ? stripTags(jsBlock.innerHTML) : null,
      unknownBlockFound: !!unknownBlock,
      unknownBlockText: unknownBlock ? unknownBlock.textContent : null,
      unknownBlockHtmlLen: unknownBlock ? unknownBlock.innerHTML.length : -1,
      unknownBlockHasSpan: unknownBlock ? unknownBlock.innerHTML.includes("<span") : false,
      unknownBlockHasHljsClass: unknownBlock ? unknownBlock.classList.contains("hljs") : false,
      plainBlockFound: !!plainBlock,
      inlineCodePreserved: !!document.querySelector(".markdown-body code:not(pre code), .markdown-body p code"),
      markdownBodyCount: document.querySelectorAll(".markdown-body").length,
      textHead: (t || "").replace(/\\s+/g, " ").slice(0, 500),
    });
  })()`;
  const evalRes = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
  const raw = evalRes?.result?.value;
  report.dom = raw ? JSON.parse(raw) : null;
  step("dom captured");

  if (report.dom) {
    const d = report.dom;
    check("无未捕获异常", report.exceptions.length === 0, `${report.exceptions.length} 条`);
    check("界面已挂载（.app-shell 存在）", d.hasShell, `rootChildCount=${d.rootChildCount}`);
    check("未落到错误边界卡片", d.errorCardShown === false);
    check("三个 Agent 均在界面中", d.triple.deepseekHarness && d.triple.codex && d.triple.deepharness, JSON.stringify(d.triple));
    check("窗口不透明（body 背景非透明）", d.bodyBg && d.bodyBg !== "rgba(0, 0, 0, 0)" && d.bodyBg !== "transparent", d.bodyBg);

    check("markdown 已渲染", d.markdownBodyCount >= 1, `${d.markdownBodyCount} 个 .markdown-body`);
    check("代码块被识别", d.codeBlockCount >= 3, `${d.codeBlockCount} 个 pre code`);
    check("JS 代码块被着色（存在 <span>）", d.jsBlockFound && d.jsBlockHasSpan);
    check("存在 hljs 主题类", d.hljsSpanCount > 0, `${d.hljsSpanCount} 个 span[class^=hljs-]`);
    check("关键字被识别（hljs-keyword）", d.keywordSpanCount > 0, `${d.keywordSpanCount} 个`);
    check(
      "着色未丢内容（去标签后含原始 token）",
      typeof d.jsBlockStrippedText === "string" &&
        d.jsBlockStrippedText.includes("const") &&
        d.jsBlockStrippedText.includes("function") &&
        d.jsBlockStrippedText.includes("return"),
      String(d.jsBlockStrippedText).slice(0, 80),
    );
    check(
      "未注册语言：不高亮、不报错、内容完整",
      d.unknownBlockFound &&
        d.unknownBlockHasSpan === false &&
        String(d.unknownBlockText ?? "").includes("made up language body"),
      `hasSpan=${d.unknownBlockHasSpan}`,
    );
    check("行内代码保留", d.inlineCodePreserved === true);
    check(
      "markdown 走异步块（Network 见 Markdown-*.js）",
      report.requests.some((r) => /Markdown-.*\.js/.test(r.url)),
      JSON.stringify(report.requests.map((r) => r.url)),
    );
    check("无 hljs 相关控制台报错", !report.console.some((c) => /highlight|hljs/i.test(c.text) && c.type === "error"));
  }

  // ── 8) 关掉欢迎弹窗，让消息区可见 ──────────────────────────────────────
  // 首次启动（无 API Key）会弹出欢迎引导，遮住消息列表。点真实按钮关闭，
  // 顺便验证「点击 → React 状态更新 → 遮罩移除」这条链路是通的。
  const dismissRes = await send(
    "Runtime.evaluate",
    {
      expression: `(() => {
        const btns = [...document.querySelectorAll("button")];
        const skip = btns.find(b => (b.innerText || "").trim() === "跳过");
        if (skip) { skip.click(); return "clicked-skip"; }
        const x = document.querySelector(".modal-mask button.btn-icon");
        if (x) { x.click(); return "clicked-close"; }
        return "no-modal";
      })()`,
      returnByValue: true,
    },
    8000,
  );
  step(`dismiss modal: ${dismissRes?.result?.value ?? "?"}`);
  await sleep(2500);

  const visRes = await send(
    "Runtime.evaluate",
    {
      expression: `(() => {
        const rectOf = (el) => {
          if (!el) return null;
          const r = el.getBoundingClientRect();
          return { w: Math.round(r.width), h: Math.round(r.height), top: Math.round(r.top) };
        };
        const blocks = [...document.querySelectorAll("pre code")];
        const first = blocks[0] ?? null;
        const cs = first ? getComputedStyle(first) : null;
        return JSON.stringify({
          modalMaskCount: document.querySelectorAll(".modal-mask").length,
          msgBubbleCount: document.querySelectorAll(".bubble").length,
          firstBlockRect: rectOf(first),
          firstBlockVisible: !!first && first.getBoundingClientRect().height > 0,
          firstBlockColor: cs ? cs.color : null,
          firstBlockFontFamily: cs ? cs.fontFamily.slice(0, 60) : null,
          // 高亮 token 的颜色必须与普通文本不同，否则「有 span 但看不出高亮」
          keywordColors: [...document.querySelectorAll("pre code span.hljs-keyword")]
            .slice(0, 4)
            .map(s => getComputedStyle(s).color),
          plainColor: first ? getComputedStyle(first.parentElement).color : null,
          // 右侧「工具」区文案：必须与 lib/agent-tools 的真实工具集一致
          panelText: (document.querySelector(".context")?.innerText || "").replace(/\\s+/g, " "),
          toggleCount: document.querySelectorAll(".context .toggle").length,
        });
      })()`,
      returnByValue: true,
    },
    8000,
  );
  const visRaw = visRes?.result?.value;
  report.visible = visRaw ? JSON.parse(visRaw) : null;
  step("visibility captured");

  if (report.visible) {
    const v = report.visible;
    check("欢迎弹窗已关闭", v.modalMaskCount === 0, `${v.modalMaskCount} 个遮罩`);
    check("消息气泡已渲染", v.msgBubbleCount >= 2, `${v.msgBubbleCount} 个 .bubble`);
    check("代码块实际可见（高度 > 0）", v.firstBlockVisible === true, JSON.stringify(v.firstBlockRect));
    check(
      "关键字着色与正文颜色不同（高亮真的可见）",
      v.keywordColors.length > 0 && v.plainColor && v.keywordColors.some((c) => c !== v.plainColor),
      `kw=${JSON.stringify(v.keywordColors)} plain=${v.plainColor}`,
    );
    // 面板文案必须与真实工具集一致：只有三个文件工具，且不再出现
    // 没有对应工具的「代码执行」「网页搜索」开关。
    check("右侧面板列出真实工具名", ["list_dir", "read_file", "write_file"].every((n) => v.panelText.includes(n)), v.panelText.slice(0, 160));
    check(
      "右侧面板不再出现无对应工具的开关文案",
      !v.panelText.includes("代码执行") && !v.panelText.includes("网页搜索"),
      v.panelText.slice(0, 160),
    );
    check("工具开关只有一个", v.toggleCount === 1, `${v.toggleCount} 个 .toggle`);
  }

  // 截图
  try {
    const shot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false }, 12000);
    if (shot?.data) {
      const buf = Buffer.from(shot.data, "base64");
      await writeFile(SHOT, buf);
      report.screenshotBytes = buf.length;
    } else {
      report.shotError = JSON.stringify(shot).slice(0, 400);
    }
  } catch (e) {
    report.shotError = String(e?.message ?? e);
  }
  step(`screenshot ${report.screenshotBytes} bytes`);

  report.passed = report.assertions.every((a) => a.ok);
}

try {
  await main();
} catch (e) {
  report.fatal = String(e?.stack ?? e);
  console.error("[fatal]", report.fatal);
} finally {
  try {
    ws?.close();
  } catch {
    /* ignore */
  }
  try {
    edge?.kill();
  } catch {
    /* ignore */
  }
  try {
    server?.close();
  } catch {
    /* ignore */
  }
  try {
    await writeFile(OUT, JSON.stringify(report, null, 2), "utf8");
  } catch (e) {
    console.error("[write-report-failed]", String(e));
  }
  const failed = report.assertions.filter((a) => !a.ok);
  console.log(
    "report ->",
    OUT,
    "| assertions:",
    `${report.assertions.length - failed.length}/${report.assertions.length}`,
    "| exceptions:",
    report.exceptions.length,
    "| fatal:",
    report.fatal ? "yes" : "no",
  );
  process.exit(report.fatal || failed.length > 0 ? 1 : 0);
}

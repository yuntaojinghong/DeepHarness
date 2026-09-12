#!/usr/bin/env node
'use strict';

/**
 * DeepHarness 插件宿主 shim（协议实现方，随包内置）。
 *
 * 这个文件由 Rust 侧以 `include_str!` 内嵌进二进制，并在运行时落地到
 * `<data>/plugin-host/host.cjs`，因此不依赖 Tauri 的资源打包配置
 * （`resources/*` 是被 gitignore 的，走资源会带来"开发能跑、打包缺文件"的坑）。
 *
 * 调用方式（由 plugins/host.rs 拉起）：
 *
 *   node --permission --allow-fs-read=<插件目录> --allow-fs-read=<本文件目录>
 *        host.cjs
 *
 * ## 与父进程的协议（stdin/stdout 上的 JSON Lines）
 *
 *   父 → 子  {"type":"invoke","pluginDir":"…","entry":"index.js",
 *            "tool":"hello","plugin":"com.x.y","args":{…},
 *            "agentId":"deepharness","workspace":"C:\\…"}
 *   子 → 父  {"type":"log","level":"info","message":"…"}
 *   子 → 父  {"type":"host","id":1,"method":"read_file","path":"C:\\…"}
 *   父 → 子  {"type":"hostResult","id":1,"ok":true,"result":"文件内容"}
 *   子 → 父  {"type":"result","ok":true,"result":{…}}   ← 终态，之后进程退出
 *   子 → 父  {"type":"fatal","error":"…"}               ← 终态（加载期失败）
 *
 * 一个 shim 进程只服务**一次**调用：拿到 invoke 就执行、发结果、退出。
 * 这样插件的全局状态不会跨调用泄漏，Rust 侧的超时也只需管一个进程。
 *
 * ## 沙箱
 *
 * 本进程以 `--permission --allow-fs-read=<插件目录>` 启动，**只能读自己
 * 的插件目录**，越界读写一律 ERR_ACCESS_DENIED。插件要碰真实文件只能
 * 通过下面的 ctx.readFile / ctx.writeFile / ctx.listDir 回到 Rust 侧，
 * 由 PermissionStore 按该 Agent 的白名单逐次校验。
 */

const path = require('node:path');
const readline = require('node:readline');

/** 单条日志的长度上限（与 Rust 侧 MAX_LOG_LEN 对齐）。 */
const MAX_LOG_CHARS = 2000;

/** 结果写出后等待 flush 的兜底时长（毫秒）。 */
const FLUSH_TIMEOUT_MS = 5000;

let hostSeq = 0;
/** 未决的宿主调用：id -> { resolve, reject } */
const pending = new Map();
/** 是否已经收到 invoke（一个进程只处理一次）。 */
let invokeSeen = false;
/** 是否已经进入终态（写过 result / fatal）。 */
let finished = false;

/**
 * 发送一条消息。`done` 为写出回调（仅终态需要）。
 * 序列化失败时降级为 fatal —— 绝不能静默丢结果。
 */
function sendLine(obj, done) {
  let payload;
  try {
    payload = JSON.stringify(obj);
  } catch (err) {
    payload = JSON.stringify({
      type: 'fatal',
      error: '插件返回值无法序列化为 JSON: ' + describe(err),
    });
  }
  if (payload === undefined) {
    payload = JSON.stringify({ type: 'fatal', error: '插件返回值无法序列化为 JSON' });
  }
  process.stdout.write(payload + '\n', done);
}

function describe(err) {
  if (err && err.stack) return String(err.stack);
  return String(err);
}

function log(level, message) {
  const text = typeof message === 'string' ? message : describe(message);
  sendLine({
    type: 'log',
    level: String(level || 'info'),
    message: text.length > MAX_LOG_CHARS ? text.slice(0, MAX_LOG_CHARS) : text,
  });
}

/** 进入终态：写出结果并退出。重复调用只生效第一次。 */
function finish(obj) {
  if (finished) return;
  finished = true;
  // 兜底：极端情况下写出回调不触发（管道已断），也要让进程退出，
  // 否则 Rust 侧只能等超时。
  const timer = setTimeout(() => process.exit(0), FLUSH_TIMEOUT_MS);
  if (typeof timer.unref === 'function') timer.unref();
  sendLine(obj, () => process.exit(0));
}

function fail(message) {
  finish({ type: 'result', ok: false, error: String(message) });
}

/** 反向调用宿主能力（文件读写一律由 Rust 侧过白名单）。 */
function hostCall(method, params) {
  const id = ++hostSeq;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    sendLine(Object.assign({ type: 'host', id, method }, params));
  });
}

const ctx = {
  agentId: '',
  workspace: '',
  /** 经宿主读一个 UTF-8 文本文件（需该 Agent 白名单授权）。 */
  readFile: (filePath) => hostCall('read_file', { path: String(filePath) }),
  /** 经宿主写一个 UTF-8 文本文件（需该 Agent 白名单授权）。 */
  writeFile: (filePath, contents) =>
    hostCall('write_file', {
      path: String(filePath),
      contents: typeof contents === 'string' ? contents : JSON.stringify(contents),
    }),
  /** 经宿主列目录，返回 [{ name, isDir, size }]。 */
  listDir: (dirPath) => hostCall('list_dir', { path: String(dirPath) }),
  log,
};

/** 把 entry 解析为插件目录内的绝对路径，并确保没有跑出插件目录。 */
function resolveEntry(pluginDir, entry) {
  const root = path.resolve(pluginDir);
  const full = path.resolve(root, String(entry || 'index.js'));
  const rel = path.relative(root, full);
  if (rel === '' || rel.startsWith('..') || path.isAbsolute(rel)) {
    throw new Error('插件入口必须位于插件目录内: ' + String(entry));
  }
  return full;
}

async function runInvoke(msg) {
  let entry;
  try {
    entry = resolveEntry(msg.pluginDir, msg.entry);
  } catch (err) {
    return fail('插件入口非法: ' + describe(err));
  }

  let mod;
  try {
    mod = require(entry);
  } catch (err) {
    return fail('加载插件入口失败（' + entry + '）: ' + describe(err));
  }

  const tools = mod && mod.tools;
  if (!tools || typeof tools !== 'object') {
    return fail('插件未导出 tools 对象（期望 module.exports = { tools: { … } }）');
  }
  const fn = tools[msg.tool];
  if (typeof fn !== 'function') {
    return fail('插件未实现工具 `' + String(msg.tool) + '`');
  }

  ctx.agentId = String(msg.agentId || '');
  ctx.workspace = String(msg.workspace || '');

  let result;
  try {
    result = await fn(msg.args === undefined ? {} : msg.args, ctx);
  } catch (err) {
    return fail('插件工具 `' + String(msg.tool) + '` 执行抛错: ' + describe(err));
  }
  finish({ type: 'result', ok: true, result: result === undefined ? null : result });
}

function handleLine(line) {
  const text = String(line).trim();
  if (text === '') return;
  let msg;
  try {
    msg = JSON.parse(text);
  } catch (err) {
    return fail('宿主协议错误：无法解析的 JSON 行');
  }
  if (!msg || typeof msg !== 'object') {
    return fail('宿主协议错误：消息必须是 JSON 对象');
  }
  switch (msg.type) {
    case 'invoke':
      // 一个进程只跑一次调用：重复 invoke 视为协议错误而非静默执行
      if (invokeSeen) return fail('宿主协议错误：一个 shim 进程只能处理一次 invoke');
      invokeSeen = true;
      void runInvoke(msg);
      return;
    case 'hostResult': {
      const waiter = pending.get(msg.id);
      if (!waiter) return;
      pending.delete(msg.id);
      if (msg.ok === false) {
        waiter.reject(new Error(String(msg.error || '宿主调用失败')));
      } else {
        waiter.resolve(msg.result);
      }
      return;
    }
    default:
      // 未知消息类型：忽略（向前兼容），不打断正在进行的调用
      return;
  }
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', handleLine);
rl.on('close', () => {
  if (!finished) fail('宿主管道关闭，插件调用未完成');
});

process.on('uncaughtException', (err) => fail('插件未捕获异常: ' + describe(err)));
process.on('unhandledRejection', (err) => fail('插件未处理的 Promise 拒绝: ' + describe(err)));

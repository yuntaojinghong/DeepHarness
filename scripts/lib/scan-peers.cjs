#!/usr/bin/env node
/**
 * 扫描 node_modules 中未被满足的同行依赖（peerDependencies）。
 *
 * 为什么需要它：`--legacy-peer-deps` 是安装 dsh 的必需开关（不加会让 npm
 * 卡死在同行依赖解析上），但它同时会跳过同行依赖的自动安装，产出一棵
 * 缺件的树——dsh 启动时会直接抛 ERR_MODULE_NOT_FOUND。
 *
 * 这里不依赖 npm 的解析能力，而是直接读本地已安装包的 package.json，
 * 按 Node 的向上查找规则判断每个声明的同行依赖能否被解析到，
 * 把缺失的项以 `名字@范围` 的规格输出，交给 npm 显式安装。
 *
 * 用法：
 *   node scan-peers.cjs <node_modules 目录>
 *
 * 输出：每行一个 `名字@范围`；无缺失则不输出。退出码 0 表示扫描成功。
 */

"use strict";

const fs = require("node:fs");
const path = require("node:path");

/** 读取一个包目录的 package.json；不可用或损坏时返回 null。 */
function readManifest(dir) {
  const manifest = path.join(dir, "package.json");
  let raw;
  try {
    raw = fs.readFileSync(manifest, "utf8");
  } catch (err) {
    if (err.code !== "ENOENT") {
      process.stderr.write(`[scan-peers] 无法读取 ${manifest}: ${err.message}\n`);
    }
    return null;
  }
  try {
    const data = JSON.parse(raw);
    if (!data || typeof data.name !== "string") return null;
    return data;
  } catch (err) {
    process.stderr.write(`[scan-peers] ${manifest} JSON 损坏: ${err.message}\n`);
    return null;
  }
}

/** 遍历 node_modules，产出每个包目录（含 @scope 与嵌套 node_modules）。 */
function* iterPackageDirs(root) {
  let entries;
  try {
    entries = fs.readdirSync(root, { withFileTypes: true });
  } catch (err) {
    if (err.code !== "ENOENT") {
      process.stderr.write(`[scan-peers] 无法读取 ${root}: ${err.message}\n`);
    }
    return;
  }
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    if (entry.name.startsWith(".") || !entry.isDirectory()) continue;
    const full = path.join(root, entry.name);
    if (entry.name.startsWith("@")) {
      let subs;
      try {
        subs = fs.readdirSync(full, { withFileTypes: true });
      } catch {
        continue;
      }
      for (const sub of subs) {
        if (sub.name.startsWith(".") || !sub.isDirectory()) continue;
        const subfull = path.join(full, sub.name);
        yield subfull;
        yield* iterPackageDirs(path.join(subfull, "node_modules"));
      }
    } else {
      yield full;
      yield* iterPackageDirs(path.join(full, "node_modules"));
    }
  }
}

/** 按 Node 的向上查找规则，从 startDir 起找 name 包所在目录。 */
function resolveFrom(startDir, name) {
  let cur = startDir;
  for (;;) {
    const cand = path.join(cur, "node_modules", ...name.split("/"));
    if (fs.existsSync(path.join(cand, "package.json"))) return cand;
    const parent = path.dirname(cur);
    if (parent === cur) return null;
    cur = parent;
  }
}

/** 把版本字符串拆成可比较的数字数组（忽略预发布后缀）。 */
function parseVersion(v) {
  const m = String(v || "").match(/(\d+)\.(\d+)\.(\d+)/);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

function cmp(a, b) {
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return 0;
}

/**
 * 粗判 version 是否满足 range。
 *
 * 只覆盖 dsh 生态里实际用到的写法（^ / ~ / >= / 精确 / *）。
 * 遇到无法识别的复杂范围时保守返回 true —— 宁可漏装也不错装，
 * 因为错装会引入版本冲突，而漏装会在后续 dsh_deps_intact 检查中暴露。
 */
function satisfies(version, range) {
  if (!range || range === "*" || range === "latest") return true;
  const v = parseVersion(version);
  if (!v) return true;

  const alternatives = String(range).includes("||") ? String(range).split("||") : [range];
  for (const alt of alternatives) {
    const part = alt.trim();
    if (!part || part === "*") return true;

    const nums = part.match(/\d+\.\d+\.\d+/);
    if (!nums) return true; // 认不出的写法，保守放行
    const base = parseVersion(nums[0]);
    if (!base) return true;

    if (part.startsWith("^")) {
      if (cmp(v, base) < 0) continue;
      // ^0.x.y 只允许同 minor，^x.y.z（x>0）允许同 major
      if (base[0] === 0 ? v[0] === 0 && v[1] === base[1] : v[0] === base[0]) return true;
    } else if (part.startsWith("~")) {
      if (v[0] === base[0] && v[1] === base[1] && cmp(v, base) >= 0) return true;
    } else if (part.startsWith(">=")) {
      if (cmp(v, base) >= 0) return true;
    } else if (/^\d/.test(part)) {
      if (v[0] === base[0] && v[1] === base[1] && v[2] === base[2]) return true;
    } else {
      return true;
    }
  }
  return false;
}

function main() {
  const root = process.argv[2];
  if (!root) {
    process.stderr.write("用法: node scan-peers.cjs <node_modules 目录>\n");
    return 2;
  }
  const absRoot = path.resolve(root);
  if (!fs.existsSync(absRoot)) {
    process.stderr.write(`[scan-peers] 目录不存在: ${absRoot}\n`);
    return 2;
  }

  // 收集树内所有包及其清单
  const packages = [];
  for (const dir of iterPackageDirs(absRoot)) {
    const manifest = readManifest(dir);
    if (manifest) packages.push({ dir, manifest });
  }

  const missing = new Map(); // name -> Set(range)

  for (const { dir, manifest } of packages) {
    const peers = manifest.peerDependencies;
    if (!peers || typeof peers !== "object") continue;
    const meta = manifest.peerDependenciesMeta || {};

    for (const [name, range] of Object.entries(peers)) {
      // 可选同行依赖不补：装了反而可能与宿主环境冲突
      if (meta[name] && meta[name].optional) continue;

      const found = resolveFrom(dir, name);
      if (!found) {
        if (!missing.has(name)) missing.set(name, new Set());
        missing.get(name).add(range);
        continue;
      }
      const installed = readManifest(found);
      if (!installed || !satisfies(installed.version, range)) {
        if (!missing.has(name)) missing.set(name, new Set());
        missing.get(name).add(range);
      }
    }
  }

  const specs = [];
  for (const [name, ranges] of [...missing.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    // 多个范围时取第一个：dsh 生态内同一包的范围基本一致
    specs.push(`${name}@${[...ranges][0]}`);
  }

  if (specs.length > 0) {
    process.stdout.write(`${specs.join(" ")}\n`);
  }
  process.stderr.write(`[scan-peers] 扫描 ${packages.length} 个包，缺失同行依赖 ${specs.length} 个\n`);
  return 0;
}

process.exit(main());

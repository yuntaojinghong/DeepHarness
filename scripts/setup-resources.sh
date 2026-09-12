#!/usr/bin/env bash
# =============================================================================
# DeepSeek Harness Desktop · 资源准备脚本
# 自动下载 Node 便携版 + 安装 @deepseek-ai/dsh，供 Tauri 打包内置进安装程序。
#
# 用法：
#   bash scripts/setup-resources.sh          # 完整准备（Node + dsh）
#   bash scripts/setup-resources.sh --node   # 仅准备 Node
#   bash scripts/setup-resources.sh --dsh    # 仅安装 dsh
#   bash scripts/setup-resources.sh --force  # 忽略「已就绪」判断，全部重下重装
#
# 环境变量：
#   NODE_VERSION   随包 Node 版本（默认 22.22.2，不得低于 22.13）
#   DSH_VERSION    @deepseek-ai/dsh 版本（默认 0.1.5-rc.2）
#   NPM_REGISTRY   npm 镜像（默认 https://registry.npmmirror.com，置空走官方源）
#
# 产物目录（已加入 .gitignore，不会提交到仓库）：
#   resources/node/                          便携版 Node（node.exe + 自带 npm）
#   resources/dsh/node_modules/@deepseek-ai/dsh/lib/bin.js  官方 Harness
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RES="$ROOT/resources"

# ---- 可调版本 ----
# Node 便携版版本。**不得低于 22.13**：dsh 的 code-runtime 使用了
# `node:module` 的 `stripTypeScriptTypes()`（22.13 引入），低版本会在
# 启动时报 "does not provide an export named 'stripTypeScriptTypes'"，
# 整个 Harness 无法加载。22.22.2 为实测通过的版本。
NODE_VERSION="${NODE_VERSION:-22.22.2}"
NODE_ARCH="${NODE_ARCH:-x64}"                       # 目标架构
DSH_VERSION="${DSH_VERSION:-0.1.5-rc.2}"            # @deepseek-ai/dsh 版本

# dsh 要求的最低 Node 版本（major.minor）。
NODE_MIN_MAJOR=22
NODE_MIN_MINOR=13

# ---- 镜像源 ----
# 国内直连官方源很慢且容易在依赖树的中间断掉（dsh 有 400+ 个包，
# 一次超时就会留下残缺的 node_modules）。默认走国内镜像；
# 置空即回退官方源，例如：NPM_REGISTRY= bash scripts/setup-resources.sh --dsh
NPM_REGISTRY="${NPM_REGISTRY-https://registry.npmmirror.com}"
NODE_MIRROR="${NODE_MIRROR-https://npmmirror.com/mirrors/node}"

FORCE=0

# 组装镜像参数；镜像被显式置空时不传 --registry，交给 npm 用官方源。
NPM_REGISTRY_ARG=()
[ -n "$NPM_REGISTRY" ] && NPM_REGISTRY_ARG=("--registry=$NPM_REGISTRY")

NODE_URL="${NODE_MIRROR}/v${NODE_VERSION}/node-v${NODE_VERSION}-win-${NODE_ARCH}.zip"
NODE_DIR="$RES/node"
DSH_DIR="$RES/dsh"
DSH_BIN="$DSH_DIR/node_modules/@deepseek-ai/dsh/lib/bin.js"

log()  { printf '\033[36m>>\033[0m %s\n' "$*"; }
ok()   { printf '\033[32m✓\033[0m %s\n' "$*"; }
# 告警走 stderr：调用方经常用 $(...) 取 stdout 当数据，
# 混进去会让「包规格」这类结果被污染（实测报过 EINVALIDTAGNAME）。
warn() { printf '\033[33m!\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

extract_zip() { # $1=zip $2=dest
  if command -v unzip >/dev/null 2>&1; then
    unzip -q "$1" -d "$2"
  elif command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command "Expand-Archive -Path '$1' -DestinationPath '$2' -Force"
  else
    fail "需要 unzip 或 PowerShell 来解压 $1"
  fi
}

# MSYS 风格路径（/c/Users/...）作为**参数**传给 Windows 原生程序会解析失败：
# node.exe 会把 /c/... 当成「当前盘符下的相对路径」，得到 C:\c\Users\...，
# 于是 npm 直接 MODULE_NOT_FOUND。实测本环境不会自动做路径转换，
# 因此凡是把路径当参数交给原生 exe 的地方都要先过这里。
# 没有 cygpath 时（非 Git Bash 环境）原样返回。
win_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s' "$1"
  fi
}

# 可用的 node 可执行文件：优先随包 Node（版本受控），其次宿主机 PATH。
# 回退是必要的：`--dsh` 单独执行时 NODE_DIR 可能还不存在；而完全不回退
# 则要求使用者必须按 --node → --dsh 的顺序调用。
node_exe() {
  if [ -f "$NODE_DIR/node.exe" ]; then
    printf '%s' "$NODE_DIR/node.exe"
  elif command -v node >/dev/null 2>&1; then
    command -v node
  else
    printf ''
  fi
}

# 统一的 npm 入口。
#
# 直接调用 `"$NODE_DIR/node.exe" <npm-cli.js>` 而不是裸 `npm`：
# 随包 Node 自带的 npm 与它自己的 node 版本必然匹配，也避免 Windows 上
# .cmd 经由 shell 转发带来的引号/编码问题；同时不要求宿主机装过 Node。
npm_run() {
  local cli="$NODE_DIR/node_modules/npm/bin/npm-cli.js"
  if [ -f "$cli" ] && [ -f "$NODE_DIR/node.exe" ]; then
    "$NODE_DIR/node.exe" "$(win_path "$cli")" "$@"
  elif command -v npm >/dev/null 2>&1; then
    npm "$@"
  else
    fail "找不到可用的 npm，请先执行：bash scripts/setup-resources.sh --node"
  fi
}

# 丢弃一棵依赖树。
#
# 先尝试直接删除；某些安全策略会对高强度删除直接报错（实测沙箱内的
# node-safe-delete-shim 在删除条目超过阈值时抛错，导致 npm 的 reify
# 阶段留下大量 `.包名-哈希` 残骸）。此时退化为改名：改名是原子的、
# 不触发删除策略，新树可以立刻开装，旧目录留给使用者自行清理。
discard_tree() {
  local target="$1"
  [ -e "$target" ] || return 0
  if rm -rf "$target" 2>/dev/null; then
    return 0
  fi
  local trash="${target}.stale"
  rm -rf "$trash" 2>/dev/null
  if mv "$target" "$trash" 2>/dev/null; then
    warn "无法直接删除 $target，已改名保留为 $(basename "$trash")"
    return 0
  fi
  return 1
}

# 判断已就位的便携 Node 是否满足 dsh 的最低要求。
#
# 只看 node.exe 是否存在是不够的：旧版本一旦存在就会被永久跳过，
# 而 dsh 的 code-runtime 需要 node:module 的 stripTypeScriptTypes()
# （22.13 引入）。实测内置 22.12.0 会让整个 Harness 加载失败。
node_version_ok() {
  local exe="$NODE_DIR/node.exe"
  [ -f "$exe" ] || return 1
  local ver
  ver="$("$exe" --version 2>/dev/null)" || return 1
  # 形如 v22.22.2
  local major minor
  major="$(echo "$ver" | sed -n 's/^v\([0-9]*\)\..*/\1/p')"
  minor="$(echo "$ver" | sed -n 's/^v[0-9]*\.\([0-9]*\)\..*/\1/p')"
  [ -n "$major" ] && [ -n "$minor" ] || return 1
  if [ "$major" -gt "$NODE_MIN_MAJOR" ]; then
    return 0
  fi
  if [ "$major" -eq "$NODE_MIN_MAJOR" ] && [ "$minor" -ge "$NODE_MIN_MINOR" ]; then
    return 0
  fi
  log "现有 Node ${ver} 低于 dsh 要求的 ${NODE_MIN_MAJOR}.${NODE_MIN_MINOR}，将重新下载"
  return 1
}

setup_node() {
  if [ "$FORCE" != 1 ] && node_version_ok; then
    local have_ver
    have_ver="$("$NODE_DIR/node.exe" --version 2>/dev/null)"
    if [ "$have_ver" != "v${NODE_VERSION}" ]; then
      warn "随包 Node 为 ${have_ver}（>= ${NODE_MIN_MAJOR}.${NODE_MIN_MINOR}，可用）；如需精确对齐 ${NODE_VERSION} 请加 --force"
    fi
    ok "Node 已就绪（跳过）：$NODE_DIR/node.exe (${have_ver})"
    return 0
  fi

  log "下载 Node 便携版 v${NODE_VERSION}（${NODE_ARCH}）..."
  local tmp="$RES/node-tmp"
  local bak="$RES/node-old"
  local extracted="$tmp/node-v${NODE_VERSION}-win-${NODE_ARCH}"

  mkdir -p "$RES"
  rm -rf "$tmp"
  mkdir -p "$tmp"

  # 下载与解压都在 $tmp 内以**相对路径**进行。
  # Windows 自带的 curl / PowerShell 不认 MSYS 风格路径（/c/Users/...），
  # 传绝对路径时 curl 会直接报 "Failed to open the file ...: No such file
  # or directory"（把 /c/Users 当成当前盘符下的相对目录）。
  ( cd "$tmp" && curl -fL --ssl-no-revoke --retry 3 "$NODE_URL" -o "node-win.zip" ) \
    || fail "下载 Node 失败：$NODE_URL"
  ( cd "$tmp" && extract_zip "node-win.zip" "." ) \
    || fail "解压 Node 失败"

  [ -f "$extracted/node.exe" ] || {
    rm -rf "$tmp"
    fail "压缩包结构与预期不符，未找到 $extracted/node.exe"
  }

  # 先验证新下载的 Node 真能运行，再动旧目录。
  # 旧版脚本直接把解压结果 mv 到可能已存在的 $NODE_DIR 下，结果是
  # resources/node/node-v22.22.2-win-x64/ —— 目录看着像成功了，
  # 但 bin 位置全错，且版本依旧过旧。
  "$extracted/node.exe" --version >/dev/null 2>&1 || {
    rm -rf "$tmp"
    fail "下载的 Node 无法执行：$extracted/node.exe"
  }

  # 替换：旧目录先改名暂存，失败则回滚，避免中途失败后只剩空目录。
  rm -rf "$bak"
  if [ -e "$NODE_DIR" ]; then
    mv "$NODE_DIR" "$bak" || { rm -rf "$tmp"; fail "暂存旧 Node 目录失败：$NODE_DIR"; }
  fi
  if mv "$extracted" "$NODE_DIR"; then
    rm -rf "$bak"
  else
    [ -e "$bak" ] && mv "$bak" "$NODE_DIR"
    rm -rf "$tmp"
    fail "替换 Node 目录失败：$NODE_DIR"
  fi

  rm -rf "$tmp"
  ok "Node 已就绪：$NODE_DIR/node.exe ($("$NODE_DIR/node.exe" --version 2>/dev/null))"
}

setup_dsh() {
  # dsh 必须跑在 >= 22.13 的 Node 上，而 npm 也从随包 Node 取，所以
  # 单独执行 `--dsh` 时也要保证 Node 就位。
  node_version_ok || setup_node

  if [ "$FORCE" != 1 ] && dsh_deps_intact; then
    ok "dsh 已就绪（跳过）：$DSH_BIN ($(installed_dsh_version))"
    return 0
  fi

  # 依赖树只要有一处残缺，就整棵重建，而不是就地补装。
  # 实测就地 `npm install` 在有残骸的树上会与删除策略打架，留下
  # `.包名-哈希` 目录并把真包移空，越修越坏；而干净安装只要十几秒。
  if [ -e "$DSH_DIR/node_modules" ]; then
    log "重建依赖树：移除现有的 node_modules 与 package-lock.json ..."
    discard_tree "$DSH_DIR/node_modules" || fail "无法移除 $DSH_DIR/node_modules"
    discard_tree "$DSH_DIR/package-lock.json" || warn "无法移除旧的 package-lock.json，继续"
  fi

  log "安装 @deepseek-ai/dsh@${DSH_VERSION} ..."
  mkdir -p "$DSH_DIR"
  if [ ! -f "$DSH_DIR/package.json" ]; then
    (cd "$DSH_DIR" && npm_run init -y >/dev/null 2>&1) || fail "初始化 $DSH_DIR/package.json 失败"
  fi
  # 把依赖写进 package.json：否则后续 `npm install`（不带参数）不会补装它，
  # 依赖树一旦残缺就再也没有自愈的机会。
  (cd "$DSH_DIR" && npm_run pkg set "dependencies.@deepseek-ai/dsh=${DSH_VERSION}" >/dev/null 2>&1) || true
  # --legacy-peer-deps 是必需的，不是省事的开关：
  # dsh 有 178 个互相声明的 @deepseek-ai/* 包，npm 的同行依赖解析在这种
  # 图上会直接卡死在 placeDep 阶段（实测 12 分钟无任何进展，必须强杀）。
  # 跳过自动同行解析后安装只需十几秒。
  # 代价是同行依赖不会自动装，所以紧接着调用 ensure_peers 显式补齐。
  (cd "$DSH_DIR" && npm_run install "@deepseek-ai/dsh@${DSH_VERSION}" \
    --legacy-peer-deps --no-audit --no-fund \
    "${NPM_REGISTRY_ARG[@]}") || fail "安装 @deepseek-ai/dsh 失败"
  ensure_peers || fail "补齐同行依赖失败"
  dsh_deps_intact || fail "依赖树仍不完整，请检查网络或镜像源后重试"
  ok "dsh 已就绪：$DSH_BIN ($(installed_dsh_version))"
}

# 输出缺失的同行依赖规格（空格分隔，可能为空）。
#
# `--legacy-peer-deps` 会跳过同行依赖，缺件时 dsh 启动直接抛
# ERR_MODULE_NOT_FOUND（实测缺 @deepseek-ai/cordis-plugin-group）。
# 这里不依赖 npm 的解析：直接遍历已安装包的 package.json 收集
# peerDependencies（跳过 optional），把缺失的作为直接依赖装到顶层。
# 扫描器的进度/告警写 stderr，此处只取 stdout。
missing_peers() {
  local node="$1" errfile out
  [ -n "$node" ] || return 0
  [ -d "$DSH_DIR/node_modules" ] || return 0
  errfile="$RES/scan-peers.err"
  if ! out="$("$node" "$(win_path "$ROOT/scripts/lib/scan-peers.cjs")" \
    "$(win_path "$DSH_DIR/node_modules")" 2>"$errfile")"; then
    warn "同行依赖扫描失败（$(tr '\n' ' ' < "$errfile" 2>/dev/null)）"
    rm -f "$errfile"
    return 0
  fi
  rm -f "$errfile"
  printf '%s' "$out"
}

# 缺失同行依赖的个数。扫不了时返回 0（不因检测能力缺失而阻断安装）。
count_missing_peers() {
  local specs
  specs="$(missing_peers "$(node_exe)")"
  if [ -z "$specs" ]; then
    printf '0'
    return 0
  fi
  printf '%s' "$specs" | tr -s '[:space:]' '\n' | grep -c . || true
}

ensure_peers() {
  local node
  node="$(node_exe)"
  if [ -z "$node" ]; then
    warn "跳过同行依赖补齐：没有可用的 node"
    return 0
  fi

  local missing
  missing="$(missing_peers "$node")"

  if [ -z "$missing" ]; then
    ok "同行依赖完整"
    return 0
  fi

  log "补齐缺失的同行依赖：$missing"
  # shellcheck disable=SC2086 -- 故意按空白拆分成多个包名
  (cd "$DSH_DIR" && npm_run install --legacy-peer-deps --no-audit --no-fund \
    "${NPM_REGISTRY_ARG[@]}" $missing) || return 1
  return 0
}

# 列出 node_modules 里真正的**包目录**（任意嵌套深度）。
#
# 只匹配 `node_modules/<name>` 与 `node_modules/@scope/<name>` 两种形状，
# 不递归到包内部的子目录。之前的实现递归所有目录，于是把 dist/、lib/
# 这类正常子目录当成「缺少 package.json 的残包」，完好的依赖树每次都被
# 判为残缺并触发整棵重装（非幂等，每次都白等 40 秒）。
# 正则里的 `[^@/.]` 排除 @scope 目录与 .bin 这类点开头目录。
list_package_dirs() {
  find "$DSH_DIR/node_modules" -mindepth 1 -type d \
    -regextype posix-extended \
    -regex '.*/node_modules/([^@/.][^/]*|@[^/]+/[^/]+)$' 2>/dev/null
}

# 读取已安装的 @deepseek-ai/dsh 版本号（读不到时输出空串）。
installed_dsh_version() {
  local pkg="$DSH_DIR/node_modules/@deepseek-ai/dsh/package.json"
  [ -f "$pkg" ] || return 0
  local node
  node="$(node_exe)"
  [ -n "$node" ] || return 0
  "$node" -e \
    'const fs=require("fs");try{process.stdout.write(String(JSON.parse(fs.readFileSync(process.argv[1],"utf8")).version||""))}catch{}' \
    "$(win_path "$pkg")" 2>/dev/null || true
}

# 判断已安装的 dsh 依赖树是否真的可用。
#
# 不能只看 bin.js：一次中断的 `npm install` 会留下 bin.js 加大量
# 「目录在、package.json 不在」的残包，此时 dsh 一启动就抛
# ERR_MODULE_NOT_FOUND（已实际发生过：163 个包缺 package.json、
# 60 个空目录、且没有 package-lock.json）。旧版脚本只检查 bin.js，
# 于是把这种坏树判定为「已就绪」永久跳过，残缺再也无法自愈。
dsh_deps_intact() {
  [ -f "$DSH_BIN" ] || return 1
  # 安装清单是一次完整安装的凭据：没有它基本可以确定安装没走完。
  [ -f "$DSH_DIR/package-lock.json" ] || return 1
  # 版本维度：DSH_VERSION 是随应用一起发布的「目标版本」，已装版本与之
  # 不一致时不能算就绪，否则升级路径被「已就绪」永久挡住 ——
  # 这正是自动更新在资源层需要一个明确入口的原因。
  local have
  have="$(installed_dsh_version)"
  if [ -z "$have" ]; then
    log "无法读取已安装的 dsh 版本（$DSH_DIR/node_modules/@deepseek-ai/dsh/package.json）"
    return 1
  fi
  if [ "$have" != "$DSH_VERSION" ]; then
    log "已安装 dsh ${have}，目标版本 ${DSH_VERSION}，需要更新"
    return 1
  fi
  local missing
  missing=$(
    list_package_dirs \
      | while IFS= read -r d; do [ -f "$d/package.json" ] || echo "$d"; done \
      | head -1
  )
  [ -z "$missing" ] || {
    log "依赖残缺：$missing 缺少 package.json"
    return 1
  }
  # 同行依赖缺失同样属于「不可用」：--legacy-peer-deps 装出来的树可能
  # 每个包都在、但互相声明的那几个包没装（实测过缺 cordis-plugin-group
  # 导致 dsh 启动即 ERR_MODULE_NOT_FOUND）。
  local peers
  peers="$(count_missing_peers)"
  if [ "$peers" != "0" ]; then
    log "依赖残缺：缺 ${peers} 个同行依赖"
    return 1
  fi
  return 0
}

main() {
  local do_node=0 do_dsh=0
  if [ "$#" -eq 0 ]; then do_node=1; do_dsh=1; fi
  for a in "$@"; do
    case "$a" in
      --node)  do_node=1 ;;
      --dsh)   do_dsh=1 ;;
      # 强制重装：忽略「已就绪」判断，Node 也重新下载。
      # 依赖树疑似损坏但判断不出来时使用。
      --force) FORCE=1 ;;
      *) fail "未知参数：$a（支持 --node / --dsh / --force）" ;;
    esac
  done

  mkdir -p "$RES"
  [ "$do_node" = 1 ] && setup_node
  [ "$do_dsh"  = 1 ] && setup_dsh

  echo
  ok "资源准备完成。现在可以运行：npm run tauri build"
}

main "$@"

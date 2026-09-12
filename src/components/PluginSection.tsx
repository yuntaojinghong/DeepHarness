import { useCallback, useEffect, useRef, useState } from "react";
import { describeError } from "../lib/deepharness";
import {
  PLUGIN_ENTRY_EXAMPLE,
  PLUGIN_MANIFEST_EXAMPLE,
  pluginDir,
  pluginInstall,
  pluginList,
  pluginPackagingHint,
  pluginSetEnabled,
  pluginSizeLimitLabel,
  pluginUninstall,
  type PluginRecord,
} from "../lib/plugins";
import { CheckIcon, FolderIcon, PlusIcon, TrashIcon } from "./Icons";

const AGENT = "deepharness";

/**
 * 插件面板：安装 / 启停 / 卸载 `.dph-plugin`，并附一份可直接照抄的写法说明。
 *
 * 只出现在 DeepHarness 面板里 —— `.dph-plugin` 扩展的是自研循环的工具层，
 * 与官方 dsh 的 cordis 插件生态互不通用（见 `src-tauri/src/plugins/mod.rs`）。
 */
export default function PluginSection() {
  const [items, setItems] = useState<PluginRecord[] | null>(null);
  const [dir, setDir] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const fileRef = useRef<HTMLInputElement | null>(null);

  const refresh = useCallback(async () => {
    setErr(null);
    try {
      const [list, root] = await Promise.all([pluginList(AGENT), pluginDir(AGENT)]);
      setItems(list);
      setDir(root);
    } catch (e) {
      console.warn("[plugins] 读取插件列表失败", e);
      setErr(describeError(e));
      setItems([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const pickFile = () => {
    setErr(null);
    setMsg(null);
    fileRef.current?.click();
  };

  const onFile = async (file: File | undefined) => {
    if (!file) return;
    setBusy("install");
    setErr(null);
    setMsg(null);
    try {
      const record = await pluginInstall(AGENT, file);
      setMsg(
        `已安装「${record.name}」${record.version}` +
          (record.valid ? "" : "（清单有问题，已标为不可用）")
      );
      await refresh();
    } catch (e) {
      console.error("[plugins] 安装插件失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
      // 允许重复选择同一个文件（否则第二次选它不会触发 change）
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const toggle = async (p: PluginRecord) => {
    setBusy(p.id);
    setErr(null);
    setMsg(null);
    try {
      await pluginSetEnabled(AGENT, p.id, !p.enabled);
      setMsg(
        p.enabled
          ? `已停用「${p.name}」`
          : `已启用「${p.name}」，运行中的 Agent 已热加载`
      );
      await refresh();
    } catch (e) {
      console.error("[plugins] 切换插件状态失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (p: PluginRecord) => {
    if (!window.confirm(`确定卸载「${p.name}」？该插件的目录会被删除，此操作不可撤销。`)) return;
    setBusy(p.id);
    setErr(null);
    setMsg(null);
    try {
      await pluginUninstall(AGENT, p.id);
      setMsg(`已卸载「${p.name}」`);
      await refresh();
    } catch (e) {
      console.error("[plugins] 卸载插件失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const copyDir = async () => {
    if (!dir) return;
    try {
      await navigator.clipboard.writeText(dir);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // 剪贴板被拒时目录本身就在界面上，用户可手动选中复制
    }
  };

  return (
    <div style={{ borderTop: "1px solid var(--border)" }}>
      <div className="section-title">插件{items ? ` · ${items.length} 个` : ""}</div>

      <div style={{ padding: "0 14px 14px" }}>
        <div style={{ fontSize: 12, lineHeight: 1.7, color: "var(--text-tertiary)" }}>
          插件给 DeepHarness 增加新工具：装好后模型规划任务时就能用它们。
          三个 Agent 的插件互不影响；插件代码跑在独立 node 进程里并被锁在该插件目录内，
          要读写文件只能经本 Agent 的授权白名单。
        </div>

        <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
          <button
            className="btn btn-sm btn-primary"
            style={{ flex: 1 }}
            onClick={pickFile}
            disabled={busy !== null}
          >
            {busy === "install" ? <span className="spinner" /> : <PlusIcon size={13} />}
            安装插件
          </button>
          <button className="btn btn-sm" onClick={() => void refresh()} disabled={busy !== null}>
            刷新
          </button>
        </div>
        <input
          ref={fileRef}
          type="file"
          accept=".dph-plugin,.zip,application/zip"
          style={{ display: "none" }}
          onChange={(e) => void onFile(e.target.files?.[0])}
        />
        <div className="dh-hint-plain" style={{ marginTop: 6 }}>
          支持 .dph-plugin（zip 包），上限 {pluginSizeLimitLabel()}。
        </div>

        {err && (
          <div className="dh-alert dh-alert-error" style={{ marginTop: 8 }}>
            {err}
          </div>
        )}
        {msg && (
          <div className="dh-alert dh-alert-info" style={{ marginTop: 8 }}>
            {msg}
          </div>
        )}

        {items && items.length === 0 && (
          <div className="dh-empty" style={{ marginTop: 10 }}>
            还没有安装插件。点上面的「安装插件」选择 .dph-plugin 文件，
            或展开下面的写法说明自己做一个。
          </div>
        )}

        {items && items.length > 0 && (
          <div className="dh-memory-list" style={{ marginTop: 10 }}>
            {items.map((p) => (
              <div className="dh-memory-item" key={p.id}>
                <div className="dh-memory-head">
                  <span style={{ fontWeight: 600, fontSize: 13 }}>{p.name}</span>
                  <span className="dh-pill info">v{p.version}</span>
                  {p.valid ? (
                    <span className="dh-pill ok">可用</span>
                  ) : (
                    <span className="dh-pill err">清单错误</span>
                  )}
                  <button
                    className="btn-icon btn-ghost"
                    style={{ width: 22, height: 22, marginLeft: "auto" }}
                    title="卸载该插件"
                    disabled={busy !== null}
                    onClick={() => void remove(p)}
                  >
                    {busy === p.id ? <span className="spinner" /> : <TrashIcon size={12} />}
                  </button>
                </div>

                {p.description && <div className="dh-memory-content">{p.description}</div>}
                {p.error && (
                  <div className="dh-alert dh-alert-error" style={{ marginTop: 6 }}>
                    {p.error}
                  </div>
                )}

                <div className="dh-memory-tags">
                  <span className="dh-tag" title="插件 id（同时是目录名）">
                    {p.id}
                  </span>
                  {p.author && <span className="dh-tag">{p.author}</span>}
                  {p.tools.map((t) => (
                    <span className="dh-tag" key={t}>
                      {t}
                    </span>
                  ))}
                </div>

                {p.homepage && (
                  <div
                    style={{
                      marginTop: 6,
                      fontSize: 11.5,
                      color: "var(--text-tertiary)",
                      wordBreak: "break-all",
                    }}
                  >
                    {p.homepage}
                  </div>
                )}

                <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
                  <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
                    {p.enabled ? "已启用" : "已停用"}
                  </span>
                  <button
                    className={`toggle ${p.enabled ? "on" : ""}`}
                    style={{ marginLeft: "auto" }}
                    aria-pressed={p.enabled}
                    disabled={busy !== null}
                    onClick={() => void toggle(p)}
                    title={
                      p.enabled
                        ? "停用后该插件工具不再提供给模型"
                        : "启用后该插件工具会提供给模型"
                    }
                  />
                </div>
              </div>
            ))}
          </div>
        )}

        {dir && (
          <div style={{ marginTop: 10 }}>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginBottom: 6 }}>
              插件目录（每个 Agent 独立）
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                border: "1px solid var(--border)",
                borderRadius: 10,
                padding: "8px 10px",
                fontSize: 11.5,
                color: "var(--text-secondary)",
              }}
            >
              <FolderIcon size={14} />
              <span
                style={{
                  flex: 1,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={dir}
              >
                {dir}
              </span>
              <button className="btn btn-sm btn-ghost" onClick={() => void copyDir()}>
                {copied ? <CheckIcon size={12} /> : "复制"}
              </button>
            </div>
          </div>
        )}

        <details style={{ marginTop: 12 }}>
          <summary
            style={{
              cursor: "pointer",
              fontSize: 12.5,
              color: "var(--text-secondary)",
              userSelect: "none",
            }}
          >
            自己写一个插件
          </summary>
          <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ fontSize: 12, lineHeight: 1.75, color: "var(--text-tertiary)" }}>
              {pluginPackagingHint()}
            </div>
            <div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 4 }}>
                manifest.json
              </div>
              <pre className="dh-code">{PLUGIN_MANIFEST_EXAMPLE}</pre>
            </div>
            <div>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 4 }}>
                index.js
              </div>
              <pre className="dh-code">{PLUGIN_ENTRY_EXAMPLE}</pre>
            </div>
            <div style={{ fontSize: 11.5, lineHeight: 1.7, color: "var(--text-tertiary)" }}>
              工具名必须是 2–48 位小写 snake_case，且不能与内置工具重名（内置工具不可被覆盖）。
              单次调用默认 20 秒超时；超时或崩溃只影响这一次调用，不会影响 Agent 本体。
            </div>
          </div>
        </details>
      </div>
    </div>
  );
}

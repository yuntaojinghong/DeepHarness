import { useCallback, useEffect, useState } from "react";
import { useAppStore } from "../store";
import { useAgentStatus } from "../lib/hooks";
import {
  agentStart,
  agentStop,
  deepharnessConfigure,
  deepharnessForget,
  deepharnessGetConfig,
  deepharnessMemoryStats,
  deepharnessRecall,
  deepharnessRemember,
  describeError,
  memoryKindLabel,
  MEMORY_KINDS,
  type Memory,
  type MemoryKind,
  type MemoryStats,
} from "../lib/deepharness";
import { PlusIcon, SearchIcon, TrashIcon } from "./Icons";
import PluginSection from "./PluginSection";

/**
 * DeepHarness 的上下文面板：Worker 状态、模型接入参数与长期记忆库。
 *
 * 这三块都只属于 DeepHarness 一个 Agent——另外两个 Agent 不共享
 * 配置、不共享记忆、不共享密钥，这也正是「三个 Agent 相互独立」的体现。
 */
export default function DeepHarnessPanel() {
  const status = useAgentStatus("deepharness");
  const refreshAgents = useAppStore((s) => s.refreshAgents);
  const readiness = useAppStore((s) => s.workerReadiness);
  const refreshWorkerReadiness = useAppStore((s) => s.refreshWorkerReadiness);
  const settings = useAppStore((s) => s.settings);
  const models = useAppStore((s) => s.models);

  const [busy, setBusy] = useState<null | "start" | "stop">(null);
  const [panelError, setPanelError] = useState<string | null>(null);

  const running = status?.state === "running";
  const selectedModel = models.find((m) => m.id === settings.defaultModelId) ?? models[0];

  const toggleWorker = async () => {
    setPanelError(null);
    setBusy(running ? "stop" : "start");
    try {
      if (running) await agentStop("deepharness");
      else await agentStart("deepharness");
      await refreshAgents();
      await refreshWorkerReadiness();
    } catch (e) {
      console.error("[deepharness] 切换 Worker 状态失败", e);
      setPanelError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="context">
      <div className="section-title">Worker</div>
      <div style={{ padding: "2px 14px 10px" }}>
        <div className="dh-kv">
          <span>进程状态</span>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <span className={`badge-dot ${running ? "ok" : status?.state === "crashed" ? "err" : ""}`} />
            {running ? "运行中" : status?.state === "crashed" ? "已崩溃" : "已停止"}
          </span>
        </div>
        <div className="dh-kv">
          <span>模型配置</span>
          <span>{readiness?.configured ? "已就绪" : "未配置"}</span>
        </div>
        <div className="dh-kv">
          <span>进程号</span>
          <span>{readiness?.pid ?? "—"}</span>
        </div>
        <div className="dh-kv">
          <span>协议版本</span>
          <span>{readiness?.version ?? "—"}</span>
        </div>
        {status?.state === "crashed" && status.reason && (
          <div className="dh-alert dh-alert-error" style={{ marginTop: 8 }}>
            {status.reason}
          </div>
        )}
        <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
          <button
            className={`btn btn-sm ${running ? "" : "btn-primary"}`}
            style={{ flex: 1 }}
            onClick={toggleWorker}
            disabled={busy !== null}
          >
            {busy ? <span className="spinner" /> : null}
            {running ? "停止" : "启动"}
          </button>
          <button className="btn btn-sm" onClick={() => void refreshWorkerReadiness()} disabled={busy !== null}>
            刷新
          </button>
        </div>
        {panelError && (
          <div className="dh-alert dh-alert-error" style={{ marginTop: 8 }}>
            {panelError}
          </div>
        )}
      </div>

      <ModelConfigForm
        defaultBaseUrl={selectedModel?.baseUrl ?? "https://api.deepseek.com/v1"}
        defaultModel={selectedModel?.model ?? "deepseek-chat"}
        fallbackApiKey={settings.apiKeys["deepseek"] ?? ""}
        onSaved={() => void refreshWorkerReadiness()}
      />

      <MemorySection running={running} configured={readiness?.configured === true} />

      <PluginSection />
    </div>
  );
}

/** 模型接入参数表单：落盘并热注入运行中的 Worker。 */
function ModelConfigForm({
  defaultBaseUrl,
  defaultModel,
  fallbackApiKey,
  onSaved,
}: {
  defaultBaseUrl: string;
  defaultModel: string;
  fallbackApiKey: string;
  onSaved: () => void;
}) {
  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [model, setModel] = useState(defaultModel);
  const [apiKey, setApiKey] = useState("");
  const [tail, setTail] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const cfg = await deepharnessGetConfig();
        if (cancelled) return;
        if (cfg.baseUrl) setBaseUrl(cfg.baseUrl);
        if (cfg.model) setModel(cfg.model);
        setTail(cfg.apiKeyTail);
      } catch (e) {
        console.warn("[deepharness] 读取模型配置失败", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const save = async () => {
    setErr(null);
    setMsg(null);
    if (!baseUrl.trim() || !model.trim()) {
      setErr("Base URL 与模型名不能为空。");
      return;
    }
    if (!apiKey.trim()) {
      setErr("请填写 API Key（出于安全考虑不回显已保存的 Key）。");
      return;
    }
    setSaving(true);
    try {
      const r = await deepharnessConfigure(baseUrl.trim(), apiKey.trim(), model.trim());
      setTail(apiKey.trim().slice(-4));
      setApiKey("");
      setMsg(r.applied ? "已保存并热注入运行中的 Worker" : "已保存，下次启动 Worker 时生效");
      onSaved();
    } catch (e) {
      console.error("[deepharness] 保存模型配置失败", e);
      setErr(describeError(e));
    } finally {
      setSaving(false);
    }
  };

  const reuse = () => {
    if (!fallbackApiKey.trim()) {
      setErr("界面上方尚未配置 DeepSeek 密钥，请先到设置中填写。");
      return;
    }
    setErr(null);
    setApiKey(fallbackApiKey);
    setMsg("已填入与对话模型相同的密钥，点击保存后生效");
  };

  return (
    <div style={{ borderTop: "1px solid var(--border)" }}>
      <div className="section-title">模型配置</div>
      <div style={{ padding: "0 14px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        <label className="dh-field">
          <span>Base URL</span>
          <input className="form-input" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.deepseek.com/v1" />
        </label>
        <label className="dh-field">
          <span>模型名</span>
          <input className="form-input" value={model} onChange={(e) => setModel(e.target.value)} placeholder="deepseek-chat" />
        </label>
        <label className="dh-field">
          <span>API Key {tail ? `（已存 · 尾号 ${tail}）` : "（未配置）"}</span>
          <input
            className="form-input"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="sk-…"
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn btn-sm btn-primary" style={{ flex: 1 }} onClick={save} disabled={saving}>
            {saving ? <span className="spinner" /> : null}
            保存配置
          </button>
          <button className="btn btn-sm" onClick={reuse} disabled={saving}>
            沿用已填密钥
          </button>
        </div>
        {err && <div className="dh-alert dh-alert-error">{err}</div>}
        {msg && <div className="dh-alert dh-alert-info">{msg}</div>}
      </div>
    </div>
  );
}

/** 长期记忆库：统计、检索、写入与删除。 */
function MemorySection({ running, configured }: { running: boolean; configured: boolean }) {
  const [stats, setStats] = useState<MemoryStats | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Memory[] | null>(null);
  const [busy, setBusy] = useState<null | "stats" | "recall" | number>(null);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const ready = running && configured;

  const loadStats = useCallback(async () => {
    if (!ready) {
      setStats(null);
      return;
    }
    setBusy("stats");
    try {
      setStats(await deepharnessMemoryStats());
    } catch (e) {
      console.warn("[deepharness] 读取记忆统计失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  }, [ready]);

  useEffect(() => {
    void loadStats();
  }, [loadStats]);

  const doRecall = async () => {
    const q = query.trim();
    if (!q) {
      setErr("请输入检索关键词。");
      return;
    }
    setErr(null);
    setMsg(null);
    setBusy("recall");
    try {
      const list = await deepharnessRecall(q, 10);
      setResults(list);
      if (!list.length) setMsg("没有匹配的记忆。");
    } catch (e) {
      console.error("[deepharness] 检索记忆失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const doForget = async (id: number) => {
    setErr(null);
    setMsg(null);
    setBusy(id);
    try {
      const existed = await deepharnessForget(id);
      setResults((prev) => (prev ? prev.filter((m) => m.id !== id) : prev));
      setMsg(existed ? "已删除该条记忆" : "该条记忆已不存在");
      await loadStats();
    } catch (e) {
      console.error("[deepharness] 删除记忆失败", e);
      setErr(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div style={{ borderTop: "1px solid var(--border)" }}>
      <div className="section-title">
        记忆库{stats ? ` · ${stats.total} 条` : ""}
      </div>

      {!ready ? (
        <div style={{ padding: "0 14px 14px", fontSize: 12.5, color: "var(--text-tertiary)", lineHeight: 1.7 }}>
          启动 Worker 并完成模型配置后，即可写入 / 检索长期记忆。
        </div>
      ) : (
        <div style={{ padding: "0 14px 14px" }}>
          <div className="dh-kv">
            <span>记忆总数</span>
            <span>{stats?.total ?? "—"}</span>
          </div>
          {stats?.byKind.map(([kind, count]) => (
            <div className="dh-kv" key={kind}>
              <span style={{ paddingLeft: 10 }}>{memoryKindLabel(kind)}</span>
              <span>{count}</span>
            </div>
          ))}

          <div style={{ position: "relative", marginTop: 10 }}>
            <span className="dh-search-icon">
              <SearchIcon size={13} />
            </span>
            <input
              className="form-input"
              style={{ paddingLeft: 30 }}
              placeholder="检索记忆关键词…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void doRecall();
              }}
            />
          </div>
          <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
            <button className="btn btn-sm" style={{ flex: 1 }} onClick={doRecall} disabled={busy !== null}>
              {busy === "recall" ? <span className="spinner" /> : <SearchIcon size={13} />}
              检索
            </button>
            <button className="btn btn-sm" onClick={() => void loadStats()} disabled={busy !== null}>
              刷新统计
            </button>
          </div>

          {err && <div className="dh-alert dh-alert-error" style={{ marginTop: 8 }}>{err}</div>}
          {msg && <div className="dh-alert dh-alert-info" style={{ marginTop: 8 }}>{msg}</div>}

          {results && results.length > 0 && (
            <div className="dh-memory-list">
              {results.map((m) => (
                <div className="dh-memory-item" key={m.id}>
                  <div className="dh-memory-head">
                    <span className="dh-pill info">{memoryKindLabel(m.kind)}</span>
                    <span className="dh-memory-importance">重要度 {m.importance.toFixed(1)}</span>
                    <button
                      className="btn-icon btn-ghost"
                      style={{ width: 22, height: 22, marginLeft: "auto" }}
                      title="删除该条记忆"
                      disabled={busy !== null}
                      onClick={() => void doForget(m.id)}
                    >
                      <TrashIcon size={12} />
                    </button>
                  </div>
                  <div className="dh-memory-content">{m.content}</div>
                  {m.tags.length > 0 && (
                    <div className="dh-memory-tags">
                      {m.tags.map((t) => (
                        <span className="dh-tag" key={t}>
                          {t}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      <RememberForm ready={ready} onSaved={() => void loadStats()} />
    </div>
  );
}

/** 手动写入一条长期记忆。 */
function RememberForm({ ready, onSaved }: { ready: boolean; onSaved: () => void }) {
  const [kind, setKind] = useState<MemoryKind>("fact");
  const [content, setContent] = useState("");
  const [tags, setTags] = useState("");
  const [importance, setImportance] = useState(0.5);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const save = async () => {
    const text = content.trim();
    if (!text) {
      setErr("记忆内容不能为空。");
      return;
    }
    setErr(null);
    setMsg(null);
    setSaving(true);
    try {
      const id = await deepharnessRemember(
        kind,
        text,
        tags
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean),
        importance
      );
      setContent("");
      setTags("");
      setMsg(`已写入记忆 #${id}`);
      onSaved();
    } catch (e) {
      console.error("[deepharness] 写入记忆失败", e);
      setErr(describeError(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div style={{ borderTop: "1px solid var(--border)", padding: "0 14px 16px" }}>
      <div className="section-title" style={{ paddingLeft: 0 }}>
        写入记忆
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <select
          className="form-input"
          value={kind}
          disabled={!ready || saving}
          onChange={(e) => setKind(e.target.value as MemoryKind)}
        >
          {MEMORY_KINDS.map((k) => (
            <option key={k} value={k}>
              {memoryKindLabel(k)}
            </option>
          ))}
        </select>
        <textarea
          className="form-input"
          rows={3}
          placeholder="要长期记住的内容…"
          value={content}
          disabled={!ready || saving}
          onChange={(e) => setContent(e.target.value)}
        />
        <input
          className="form-input"
          placeholder="标签（逗号分隔）"
          value={tags}
          disabled={!ready || saving}
          onChange={(e) => setTags(e.target.value)}
        />
        <label className="dh-field">
          <span>重要度 {importance.toFixed(1)}</span>
          <input
            type="range"
            min={0}
            max={1}
            step={0.1}
            value={importance}
            disabled={!ready || saving}
            onChange={(e) => setImportance(Number(e.target.value))}
          />
        </label>
        <button className="btn btn-sm btn-primary" onClick={save} disabled={!ready || saving}>
          {saving ? <span className="spinner" /> : <PlusIcon size={13} />}
          写入记忆
        </button>
        {!ready && (
          <div className="dh-hint-plain">Worker 未就绪时不可写入。</div>
        )}
        {err && <div className="dh-alert dh-alert-error">{err}</div>}
        {msg && <div className="dh-alert dh-alert-info">{msg}</div>}
      </div>
    </div>
  );
}

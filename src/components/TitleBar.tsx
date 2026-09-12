import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import {
  ChevronDownIcon,
  ExternalLinkIcon,
  GearIcon,
  HeartIcon,
  MoonIcon,
  PanelLeftIcon,
  PanelRightIcon,
  SunIcon,
} from "./Icons";
import LogoMark from "./Logo";
import ModelIcon from "./ModelIcon";
import { AGENT_META, agentOpenWebUi, agentStateLabel, agentStateTone, agentStart, agentStop, describeError } from "../lib/deepharness";

export default function TitleBar() {
  const models = useAppStore((s) => s.models);
  const selectedId = useAppStore((s) => s.settings.defaultModelId);
  const selectModel = useAppStore((s) => s.selectModel);
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const env = useAppStore((s) => s.env);
  const sidebarOpen = useAppStore((s) => s.sidebarOpen);
  const setSidebarOpen = useAppStore((s) => s.setSidebarOpen);
  const contextOpen = useAppStore((s) => s.contextOpen);
  const setContextOpen = useAppStore((s) => s.setContextOpen);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const setEnvOpen = useAppStore((s) => s.setEnvOpen);
  const setSupportOpen = useAppStore((s) => s.setSupportOpen);

  const activeAgent = useAppStore((s) => s.activeAgent);
  const agents = useAppStore((s) => s.agents);
  const refreshAgents = useAppStore((s) => s.refreshAgents);

  const [menuOpen, setMenuOpen] = useState(false);
  const [powerBusy, setPowerBusy] = useState(false);
  const [powerError, setPowerError] = useState<string | null>(null);
  const [webUiBusy, setWebUiBusy] = useState(false);
  const [webUiHint, setWebUiHint] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const selected = models.find((m) => m.id === selectedId) ?? models[0];
  const meta = AGENT_META[activeAgent];
  const overview = agents.find((a) => a.id === activeAgent);
  const status = overview?.status;
  const running = status?.state === "running";

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  // 切换 Agent 后清掉上一个 Agent 的错误提示，避免张冠李戴。
  useEffect(() => {
    setWebUiHint(null);
  }, [activeAgent]);

  const toggleAgent = async () => {
    if (powerBusy) return;
    setPowerBusy(true);
    setPowerError(null);
    try {
      if (running) await agentStop(activeAgent);
      else await agentStart(activeAgent);
      await refreshAgents();
    } catch (e) {
      console.error("[agent] 切换运行状态失败", e);
      setPowerError(describeError(e));
      // 失败后仍刷新一次，让徽标回到真实状态（例如启动中 → 已崩溃）。
      await refreshAgents();
    } finally {
      setPowerBusy(false);
    }
  };

  // 官方 Web UI 只在 Harness 运行起来后才有意义；未运行时按钮禁用并说明原因。
  //
  // 打开动作交给后端：dsh 0.1.5 起 Web UI 需要每次启动重新生成的一次性
  // token，而它在进程起来几秒后才打印出来。前端 await 之后再
  // `window.open` 既可能被弹窗拦截，也可能拿到一个无法再改地址的新窗口。
  const webUi = meta.webUiUrl;
  const canOpenWebUi = Boolean(webUi) && running;

  const openWebUi = async () => {
    if (webUiBusy || !canOpenWebUi) return;
    setWebUiBusy(true);
    setWebUiHint(null);
    try {
      // 成功后不再提示：浏览器弹出来本身就是反馈。
      await agentOpenWebUi(activeAgent);
    } catch (e) {
      setWebUiHint(describeError(e));
    } finally {
      setWebUiBusy(false);
    }
  };

  const envState = !env ? "busy" : [env.python, env.node, env.git].every((r) => r?.installed) ? "ok" : "warn";
  const envLabel = !env ? "环境检测中…" : [env.python, env.node, env.git].every((r) => r?.installed) ? "环境就绪" : "环境待补齐";

  return (
    <div className="titlebar">
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <LogoMark size={30} />
        <span style={{ display: "flex", flexDirection: "column", lineHeight: 1.15 }}>
          <span style={{ fontSize: 14.5, fontWeight: 600, letterSpacing: 0.2 }}>
            {overview?.displayName ?? meta.name}
          </span>
          <span style={{ fontSize: 10.5, color: "var(--text-tertiary)" }}>{meta.tagline}</span>
        </span>
      </div>

      <div style={{ flex: 1 }} />

      <button className="btn-icon btn-ghost" title="切换侧边栏" onClick={() => setSidebarOpen(!sidebarOpen)}>
        <PanelLeftIcon />
      </button>

      {!meta.hasTaskConsole && (
        <div ref={menuRef} style={{ position: "relative" }}>
          <button className="btn" style={{ minWidth: 180, justifyContent: "space-between" }} onClick={() => setMenuOpen(!menuOpen)}>
            <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <ModelIcon provider={selected?.provider ?? "custom"} size={20} />
              <span style={{ fontWeight: 500 }}>{selected?.name ?? "选择模型"}</span>
            </span>
            <ChevronDownIcon size={14} />
          </button>
          {menuOpen && (
            <div className="menu" style={{ top: "calc(100% + 6px)", left: 0 }}>
              <div className="section-title" style={{ padding: "4px 10px" }}>切换模型</div>
              {models.map((m) => (
                <button
                  key={m.id}
                  className={`menu-item ${m.id === selectedId ? "active" : ""}`}
                  onClick={() => {
                    selectModel(m.id);
                    setMenuOpen(false);
                  }}
                >
                  <ModelIcon provider={m.provider} size={24} />
                  <span style={{ flex: 1 }}>
                    <div style={{ fontSize: 13 }}>{m.name}</div>
                    <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 1 }}>
                      {m.contextWindow >= 100000 ? `${(m.contextWindow / 1000).toFixed(0)}K 上下文` : `${m.contextWindow / 1000}K`}
                      {m.supportsTools ? " · 工具调用" : ""}
                    </div>
                  </span>
                </button>
              ))}
              <div className="menu-sep" />
              <button className="menu-item" onClick={() => setSettingsOpen(true)}>
                <GearIcon size={14} /> 管理模型与密钥…
              </button>
            </div>
          )}
        </div>
      )}

      <button
        className="badge"
        style={{ cursor: "pointer" }}
        title={powerError ?? `${meta.name}：${agentStateLabel(status)}`}
        onClick={toggleAgent}
        disabled={powerBusy}
      >
        <span className={`badge-dot ${powerBusy ? "busy" : agentStateTone(status)}`} />
        {powerBusy ? "切换中…" : running ? "运行中 · 点击停止" : "已停止 · 点击启动"}
      </button>

      {webUi && (
        <button
          className="btn-icon btn-ghost"
          title={
            !canOpenWebUi
              ? "启动后可用"
              : webUiBusy
                ? "正在等待官方界面就绪…"
                : "在浏览器中打开官方 Web UI"
          }
          disabled={!canOpenWebUi || webUiBusy}
          onClick={openWebUi}
        >
          <ExternalLinkIcon />
        </button>
      )}

      {(powerError || webUiHint) && (
        <span className="titlebar-error" title={powerError ?? webUiHint ?? ""}>
          {powerError ?? webUiHint}
        </span>
      )}

      <button className="badge" style={{ cursor: "pointer" }} onClick={() => setEnvOpen(true)} title="环境面板">
        <span className={`badge-dot ${envState}`} />
        {envLabel}
      </button>

      <button className="btn-icon btn-ghost" title="切换主题" onClick={() => setSettings({ theme: settings.theme === "light" ? "dark" : "light" })}>
        {settings.theme === "light" ? <MoonIcon /> : <SunIcon />}
      </button>

      <button className="btn-icon btn-ghost" title="右侧面板" onClick={() => setContextOpen(!contextOpen)}>
        <PanelRightIcon />
      </button>

      <button className="btn-icon btn-ghost" title="支持作者" onClick={() => setSupportOpen(true)}>
        <HeartIcon />
      </button>

      <button className="btn-icon btn-ghost" title="设置" onClick={() => setSettingsOpen(true)}>
        <GearIcon />
      </button>
    </div>
  );
}

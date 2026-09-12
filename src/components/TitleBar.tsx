import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import { ChevronDownIcon, GearIcon, MoonIcon, PanelLeftIcon, PanelRightIcon, SunIcon } from "./Icons";
import LogoMark from "./Logo";
import ModelIcon from "./ModelIcon";

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

  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const selected = models.find((m) => m.id === selectedId) ?? models[0];

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  const envState = !env ? "busy" : [env.python, env.node, env.git].every((r) => r?.installed) ? "ok" : "warn";
  const envLabel = !env ? "环境检测中…" : [env.python, env.node, env.git].every((r) => r?.installed) ? "环境就绪" : "环境待补齐";

  return (
    <div className="titlebar">
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <LogoMark size={30} />
        <span style={{ fontSize: 14.5, fontWeight: 600, letterSpacing: 0.2 }}>
          DeepHarness
        </span>
      </div>

      <div style={{ flex: 1 }} />

      <button className="btn-icon btn-ghost" title="切换侧边栏" onClick={() => setSidebarOpen(!sidebarOpen)}>
        <PanelLeftIcon />
      </button>

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

      <button className="btn-icon btn-ghost" title="设置" onClick={() => setSettingsOpen(true)}>
        <GearIcon />
      </button>
    </div>
  );
}

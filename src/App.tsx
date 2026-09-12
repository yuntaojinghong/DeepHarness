import { useEffect, useState } from "react";
import { useAppStore } from "./store";
import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import ChatArea from "./components/ChatArea";
import ContextPanel from "./components/ContextPanel";
import SettingsModal from "./components/SettingsModal";
import EnvModal from "./components/EnvModal";
import WelcomeModal from "./components/WelcomeModal";
import SplashScreen from "./components/SplashScreen";
import { isTauri } from "./lib/env";

export default function App() {
  const theme = useAppStore((s) => s.settings.theme);
  const fontSize = useAppStore((s) => s.settings.fontSize);
  const sidebarOpen = useAppStore((s) => s.sidebarOpen);
  const contextOpen = useAppStore((s) => s.contextOpen);
  const settingsOpen = useAppStore((s) => s.settingsOpen);
  const envOpen = useAppStore((s) => s.envOpen);
  const setEnv = useAppStore((s) => s.setEnv);
  const hydrate = useAppStore((s) => s.hydrate);
  const refreshModels = useAppStore((s) => s.refreshModels);
  const welcomeOpen = useAppStore((s) => s.welcomeOpen);
  const setWelcomeOpen = useAppStore((s) => s.setWelcomeOpen);
  const hasKey = useAppStore((s) => Object.values(s.settings.apiKeys).some((k) => k.trim()));

  const [booting, setBooting] = useState(true);
  const [leaving, setLeaving] = useState(false);
  const [bootStatus, setBootStatus] = useState("正在启动 DeepHarness…");
  const [bootError, setBootError] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<{ version: string; url: string } | null>(null);

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  useEffect(() => {
    const resolved =
      theme === "system"
        ? window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light"
        : theme;
    document.documentElement.setAttribute("data-theme", resolved);
  }, [theme]);

  useEffect(() => {
    document.documentElement.style.setProperty("font-size", `${fontSize}px`);
  }, [fontSize]);

  useEffect(() => {
    import("./lib/env")
      .then((m) => m.checkEnv())
      .then(setEnv)
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!hasKey) setWelcomeOpen(true);
  }, [hasKey, setWelcomeOpen]);

  // 官方模型实时拉取（已配置 Key 时启动即刷新）
  useEffect(() => {
    if (hasKey) refreshModels();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasKey]);

  // 启动流程：Tauri 模式启动内置 DeepSeek Harness 服务后跳转；浏览器预览则直接进入
  useEffect(() => {
    if (!isTauri()) {
      const t = setTimeout(() => setLeaving(true), 1300);
      return () => clearTimeout(t);
    }
    let cancelled = false;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        setBootStatus("正在启动 DeepHarness 服务…");
        await invoke("start_dsh");
        if (cancelled) return;
        setBootStatus("服务已就绪，正在进入…");
        window.location.href = "http://127.0.0.1:3080";
      } catch (e) {
        if (!cancelled) setBootError(`启动失败：${String(e)}`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!leaving) return;
    const t = setTimeout(() => setBooting(false), 450);
    return () => clearTimeout(t);
  }, [leaving]);

  // 检查新版本
  useEffect(() => {
    if (!isTauri()) return;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { getVersion } = await import("@tauri-apps/api/app");
        const latest = await invoke<{ version: string; url: string } | null>("check_update");
        const current = await getVersion();
        if (latest && latest.version !== `v${current}`) {
          setUpdateInfo(latest);
        }
      } catch {
        /* 检查更新失败则静默跳过 */
      }
    })();
  }, []);

  if (booting) {
    return (
      <SplashScreen
        leaving={leaving}
        status={bootStatus}
        error={bootError}
        onRetry={() => window.location.reload()}
      />
    );
  }

  return (
    <div className="app-shell">
      <TitleBar />
      <div className="body">
        {sidebarOpen && <Sidebar />}
        <ChatArea />
        {contextOpen && <ContextPanel />}
      </div>

      {updateInfo && (
        <div className="update-banner">
          <span>
            发现新版本 <b>{updateInfo.version}</b>
          </span>
          <a href={updateInfo.url} target="_blank" rel="noreferrer">
            前往下载
          </a>
          <button className="btn-icon btn-ghost" onClick={() => setUpdateInfo(null)} title="关闭">
            ×
          </button>
        </div>
      )}

      {welcomeOpen && <WelcomeModal />}
      {settingsOpen && <SettingsModal />}
      {envOpen && <EnvModal />}
    </div>
  );
}

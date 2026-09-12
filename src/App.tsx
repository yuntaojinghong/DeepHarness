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
import { checkEnv, isTauri } from "./lib/env";

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
  const refreshAgents = useAppStore((s) => s.refreshAgents);
  const activeAgent = useAppStore((s) => s.activeAgent);
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
    if (!hasKey) setWelcomeOpen(true);
  }, [hasKey, setWelcomeOpen]);

  // 官方模型实时拉取（已配置 Key 时启动即刷新）
  useEffect(() => {
    if (hasKey) refreshModels();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasKey]);

  // 运行中的 Agent 会因崩溃 / 退出而变更状态，按固定间隔轻量轮询概览。
  // 未运行时开销仅一次 IPC，不影响界面响应。DeepHarness 激活时额外刷新
  // Worker 就绪状态——统一在这里轮询，任务控制台与右侧面板只读结果，
  // 避免同一个命令被并发调用多次。
  useEffect(() => {
    if (!isTauri()) return;
    const tick = () => {
      void refreshAgents();
      const { activeAgent: current, refreshWorkerReadiness } = useAppStore.getState();
      if (current === "deepharness") void refreshWorkerReadiness();
    };
    tick();
    const t = setInterval(tick, 5000);
    return () => clearInterval(t);
  }, [refreshAgents]);

  // 启动流程：只做本地初始化后进入自研 React 主界面。
  //
  // 三个 Agent 全部在本界面内使用；官方 Web UI 不再作为启动跳转目标，
  // 而是由用户在需要时从 Agent 卡片中自行打开。初始化失败不阻塞进入，
  // 只在启动页提示，保证零配置用户也能直接看到界面。
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const enterAfter = (ms: number) => {
      timer = setTimeout(() => {
        if (!cancelled) setLeaving(true);
      }, ms);
    };

    (async () => {
      if (!isTauri()) {
        setBootStatus("浏览器预览模式 · 桌面能力不可用");
        enterAfter(600);
        return;
      }
      try {
        setBootStatus("正在检测本机运行环境…");
        const env = await checkEnv();
        if (cancelled) return;
        setEnv(env);

        setBootStatus("正在读取 Agent 状态…");
        await refreshAgents();
        if (cancelled) return;

        setBootStatus("正在进入工作台…");
        enterAfter(240);
      } catch (e) {
        if (!cancelled) setBootError(`初始化失败：${String(e)}`);
      }
    })();

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [refreshAgents, setEnv]);

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
    <div className={`app-shell agent-${activeAgent}`}>
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

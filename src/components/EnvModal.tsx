import { useEffect, useState } from "react";
import { useAppStore } from "../store";
import { checkEnv, isTauri } from "../lib/env";
import { CloseIcon, CheckIcon, CpuIcon, PythonIcon, NodeIcon, GitIcon } from "./Icons";

type EnvItem = { installed?: boolean; version?: string; path?: string };

export default function EnvModal() {
  const setEnvOpen = useAppStore((s) => s.setEnvOpen);
  const env = useAppStore((s) => s.env);
  const setEnv = useAppStore((s) => s.setEnv);
  const [checking, setChecking] = useState(false);

  const refresh = async () => {
    setChecking(true);
    try {
      const r = await checkEnv();
      setEnv(r);
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    if (!env) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const Row = ({ label, icon, info }: { label: string; icon: React.ReactNode; info?: EnvItem | null }) => {
    const ok = info ? info.installed : null;
    return (
      <div className="env-row">
        <div style={{ width: 38, height: 38, flexShrink: 0, borderRadius: 9, overflow: "hidden" }}>{icon}</div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 13.5, fontWeight: 500 }}>{label}</span>
            {ok === true && (
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 11.5, color: "var(--success)" }}>
                <CheckIcon size={12} /> 已就绪
              </span>
            )}
            {ok === false && <span style={{ fontSize: 11.5, color: "var(--warning)" }}>缺失 · 需要补齐</span>}
            {ok === null && <span style={{ fontSize: 11.5, color: "var(--text-tertiary)" }}>未检测</span>}
          </div>
          {info?.version && <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 2 }}>版本 {info.version}</div>}
          {info?.path && (
            <div style={{ fontSize: 11.5, color: "var(--text-tertiary)", marginTop: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {info.path}
            </div>
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="modal-mask" onMouseDown={(e) => e.target === e.currentTarget && setEnvOpen(false)}>
      <div className="modal" style={{ width: 560 }}>
        <div className="modal-head">
          <span style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 15, fontWeight: 500 }}>
            <CpuIcon size={16} /> 运行环境
          </span>
          <button className="btn-icon btn-ghost" onClick={() => setEnvOpen(false)}>
            <CloseIcon />
          </button>
        </div>

        <div className="modal-body">
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14 }}>
            <div style={{ flex: 1, fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.6 }}>
              这里检测你电脑上的运行环境。缺失的运行时，应用会在首次启动时自动下载<b>便携版</b>到应用目录，不污染系统、无需管理员权限。
            </div>
            <button className="btn btn-sm" onClick={refresh} disabled={checking}>
              {checking ? "检测中…" : "重新检测"}
            </button>
          </div>

          <Row label="Python" icon={<PythonIcon size={38} />} info={env?.python} />
          <Row label="Node.js" icon={<NodeIcon size={38} />} info={env?.node} />
          <Row label="Git" icon={<GitIcon size={38} />} info={env?.git} />

          {env && (
            <div style={{ marginTop: 14, padding: "10px 14px", borderRadius: 10, background: "var(--bg-secondary)", fontSize: 12, color: "var(--text-secondary)", lineHeight: 1.7 }}>
              系统：{env.os} · 架构：{env.arch}
              <br />
              {env.portable ? "便携模式：数据跟随应用目录" : "数据目录"}：{env.dataDir}
            </div>
          )}

          {!isTauri() && (
            <div style={{ marginTop: 14, padding: "10px 14px", borderRadius: 10, background: "var(--warning-weak)", color: "var(--warning)", fontSize: 12.5, lineHeight: 1.6 }}>
              当前运行在浏览器预览模式，仅展示界面。打包后的桌面应用（<span style={{ fontFamily: "ui-monospace, monospace" }}>npm run tauri dev</span>）会通过系统能力做真实检测与自动补齐。
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

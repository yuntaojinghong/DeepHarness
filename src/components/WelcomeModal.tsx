import { useState } from "react";
import { useAppStore } from "../store";
import LogoMark from "./Logo";
import { CloseIcon, SparkIcon } from "./Icons";

export default function WelcomeModal() {
  const setApiKey = useAppStore((s) => s.setApiKey);
  const setSettings = useAppStore((s) => s.setSettings);
  const setWelcomeOpen = useAppStore((s) => s.setWelcomeOpen);
  const existingKey = useAppStore((s) => s.settings.apiKeys["deepseek"] || "");

  const [key, setKey] = useState(existingKey);
  const [workspace, setWorkspace] = useState("");

  const start = () => {
    if (key.trim()) setApiKey("deepseek", key.trim());
    if (workspace.trim()) setSettings({ workspace: workspace.trim() });
    setWelcomeOpen(false);
  };

  return (
    <div className="modal-mask">
      <div className="modal" style={{ width: 540 }}>
        <div className="modal-head">
          <span style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 15, fontWeight: 600 }}>
            <LogoMark size={26} radius={8} /> 欢迎使用 DeepHarness
          </span>
          <button className="btn-icon btn-ghost" onClick={() => setWelcomeOpen(false)}>
            <CloseIcon />
          </button>
        </div>

        <div className="modal-body">
          <div style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.7, marginBottom: 18 }}>
            两步配置，马上开始。密钥只保存在本机，不会上传。
          </div>

          <div className="form-row">
            <label className="form-label">① 填写 DeepSeek API Key</label>
            <input
              className="form-input"
              type="password"
              placeholder="sk-…（platform.deepseek.com 申请）"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <div className="form-hint">用于 DeepSeek V3 / R1 系列模型。也可稍后在「设置」里补充其它服务。</div>
          </div>

          <div className="form-row">
            <label className="form-label">② 选择工作目录（可选）</label>
            <input
              className="form-input"
              placeholder="AI 允许「干活」的目录，如 D:\MyProjects"
              value={workspace}
              onChange={(e) => setWorkspace(e.target.value)}
            />
            <div className="form-hint">留空则使用默认目录；这是 AI 读取/整理文件的地盘。</div>
          </div>
        </div>

        <div className="modal-foot">
          <button className="btn" onClick={() => setWelcomeOpen(false)}>跳过</button>
          <button className="btn btn-primary" onClick={start}>
            <SparkIcon size={14} /> 开始使用
          </button>
        </div>
      </div>
    </div>
  );
}

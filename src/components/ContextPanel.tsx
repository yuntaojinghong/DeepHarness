import { useAppStore } from "../store";
import { useActiveConversation } from "../lib/hooks";
import { AGENT_META } from "../lib/deepharness";
import DeepHarnessPanel from "./DeepHarnessPanel";
import { AGENT_TOOL_NAMES } from "../lib/agent-tools";
import { FolderIcon } from "./Icons";

export default function ContextPanel() {
  const conv = useActiveConversation();
  const tools = useAppStore((s) => s.tools);
  const setTools = useAppStore((s) => s.setTools);
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const models = useAppStore((s) => s.models);
  const activeAgent = useAppStore((s) => s.activeAgent);

  // 任务型 Agent 的右侧面板是 Worker / 模型配置 / 记忆库，与对话无关。
  if (AGENT_META[activeAgent].hasTaskConsole) {
    return <DeepHarnessPanel />;
  }

  const model = models.find((m) => m.id === (conv?.modelId ?? settings.defaultModelId));
  const tokens = conv ? conv.messages.reduce((acc, m) => acc + Math.ceil(m.content.length / 2), 0) : 0;

  const Toggle = ({ on, onClick }: { on: boolean; onClick: () => void }) => (
    <button className={`toggle ${on ? "on" : ""}`} onClick={onClick} aria-pressed={on} />
  );

  return (
    <div className="context">
      <div className="section-title">上下文</div>

      <div style={{ padding: "2px 14px 10px" }}>
        <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginBottom: 6 }}>工作区</div>
        <div style={{ display: "flex", alignItems: "center", gap: 8, border: "1px solid var(--border)", borderRadius: 10, padding: "8px 10px", fontSize: 12, color: "var(--text-secondary)" }}>
          <FolderIcon size={14} />
          <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {settings.workspace || "未设置（默认使用项目目录）"}
          </span>
        </div>
      </div>

      <div style={{ padding: "0 14px 10px" }}>
        <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginBottom: 8 }}>工具（模型支持时自动启用）</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 13 }}>
            <span>文件工具</span>
            <Toggle on={tools.fileTools} onClick={() => setTools({ fileTools: !tools.fileTools })} />
          </div>
          {/* 工具名从 lib/agent-tools 派生，保证界面与实际下发的工具集不会各说各话 */}
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            {AGENT_TOOL_NAMES.map((name) => (
              <span
                key={name}
                style={{
                  fontFamily: "ui-monospace, monospace",
                  fontSize: 11,
                  color: "var(--text-tertiary)",
                  border: "1px solid var(--border)",
                  borderRadius: 6,
                  padding: "1px 6px",
                }}
              >
                {name}
              </span>
            ))}
          </div>
          <div style={{ fontSize: 11.5, lineHeight: 1.6, color: "var(--text-tertiary)" }}>
            文件操作按当前 Agent 的授权目录逐次校验。出于安全设计，不提供命令执行与网络访问能力。
          </div>
        </div>
      </div>

      <div style={{ padding: "0 14px 10px", borderTop: "1px solid var(--border)", marginTop: 4 }}>
        <div className="section-title" style={{ paddingLeft: 0 }}>会话</div>
        <div style={{ fontSize: 12.5, lineHeight: 2, color: "var(--text-secondary)" }}>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>消息数</span>
            <span style={{ fontFamily: "ui-monospace, monospace" }}>{conv?.messages.length ?? 0}</span>
          </div>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>约消耗 token</span>
            <span style={{ fontFamily: "ui-monospace, monospace" }}>{tokens.toLocaleString()}</span>
          </div>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>当前模型</span>
            <span>{model?.name ?? "—"}</span>
          </div>
        </div>
      </div>

      <div style={{ padding: "0 14px 12px" }}>
        <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginBottom: 6 }}>显示密度</div>
        <div style={{ display: "flex", gap: 8 }}>
          {(["comfortable", "compact"] as const).map((d) => (
            <button
              key={d}
              className={`btn btn-sm ${settings.density === d ? "btn-primary" : ""}`}
              onClick={() => setSettings({ density: d })}
            >
              {d === "comfortable" ? "宽松" : "紧凑"}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

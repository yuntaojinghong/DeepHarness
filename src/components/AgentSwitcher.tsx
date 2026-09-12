import { useAppStore } from "../store";
import { AGENT_IDS, type AgentId } from "../lib/env";
import { AGENT_META, agentStateLabel, agentStateTone } from "../lib/deepharness";

/**
 * 三个内置 Agent 的切换器。
 *
 * 放在侧栏顶部：切换后会话列表、主视图与右侧面板全部随之切换，
 * 三个 Agent 之间不共享会话、授权、配置与记忆。
 */
export default function AgentSwitcher() {
  const activeAgent = useAppStore((s) => s.activeAgent);
  const setActiveAgent = useAppStore((s) => s.setActiveAgent);
  const agents = useAppStore((s) => s.agents);
  const agentsError = useAppStore((s) => s.agentsError);

  return (
    <div className="agent-switcher">
      <div className="section-title" style={{ padding: "12px 12px 6px" }}>
        Agent
      </div>
      {AGENT_IDS.map((id: AgentId) => {
        const meta = AGENT_META[id];
        const status = agents.find((a) => a.id === id)?.status;
        const active = id === activeAgent;
        return (
          <button
            key={id}
            className={`agent-card ${active ? "active" : ""}`}
            onClick={() => setActiveAgent(id)}
            title={meta.tagline}
          >
            <span className={`badge-dot ${agentStateTone(status)}`} />
            <span style={{ flex: 1, minWidth: 0 }}>
              <span className="agent-card-name">{agents.find((a) => a.id === id)?.displayName ?? meta.name}</span>
              <span className="agent-card-sub">{agentStateLabel(status)}</span>
            </span>
            {meta.hasTaskConsole && <span className="agent-card-tag">原生</span>}
          </button>
        );
      })}
      {agentsError && (
        <div className="agent-switcher-error" title={agentsError}>
          状态读取失败
        </div>
      )}
    </div>
  );
}

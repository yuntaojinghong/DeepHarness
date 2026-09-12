import { useAppStore } from "../store";
import { AGENT_META } from "../lib/deepharness";
import AgentSwitcher from "./AgentSwitcher";
import { CpuIcon, PlusIcon, SearchIcon, TrashIcon } from "./Icons";

export default function Sidebar() {
  const activeAgent = useAppStore((s) => s.activeAgent);
  const conversations = useAppStore((s) => s.searchConversations());
  const activeId = useAppStore((s) => s.activeId);
  const setActive = useAppStore((s) => s.setActive);
  const newConversation = useAppStore((s) => s.newConversation);
  const deleteConversation = useAppStore((s) => s.deleteConversation);
  const renameConversation = useAppStore((s) => s.renameConversation);
  const searchQuery = useAppStore((s) => s.searchQuery);
  const setSearchQuery = useAppStore((s) => s.setSearchQuery);
  const setContextOpen = useAppStore((s) => s.setContextOpen);

  const handleNew = () => {
    newConversation();
  };

  const handleDelete = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    if (window.confirm("删除该会话？此操作不可恢复。")) deleteConversation(id);
  };

  const handleRename = (e: React.MouseEvent, id: string, current: string) => {
    e.stopPropagation();
    const title = window.prompt("会话名称：", current);
    if (title && title.trim()) renameConversation(id, title.trim());
  };

  // DeepHarness 是任务型 Agent，没有「对话」概念：侧栏改为任务控制台的
  // 说明与快捷入口，而不是一个永远为空的会话列表。
  const isTaskAgent = AGENT_META[activeAgent].hasTaskConsole;

  return (
    <div className="sidebar">
      <AgentSwitcher />

      {isTaskAgent ? (
        <>
          <div className="dh-sidebar-note">
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontWeight: 500, color: "var(--text)" }}>
              <CpuIcon size={14} />
              任务型 Agent
            </div>
            <p>
              这个 Agent 不按「聊天」工作：你给出目标，它先生成结构化计划，再逐步调用工具执行，
              每步都做一次反思，最后把可复用的结论写入长期记忆。
            </p>
            <p>全部任务结果都展示在中间的运行面板，不占用会话列表。</p>
          </div>
          <div style={{ padding: "0 12px" }}>
            <button className="btn btn-sm" style={{ width: "100%" }} onClick={() => setContextOpen(true)}>
              打开模型与记忆面板
            </button>
          </div>
        </>
      ) : (
        <>
          <div style={{ padding: "8px 12px 4px" }}>
            <button className="btn btn-primary" style={{ width: "100%", padding: "9px" }} onClick={handleNew}>
              <PlusIcon size={15} /> 新会话
            </button>
          </div>

          <div style={{ padding: "8px 12px" }}>
            <div style={{ position: "relative" }}>
              <span style={{ position: "absolute", left: 10, top: "50%", transform: "translateY(-50%)", color: "var(--text-tertiary)", display: "flex" }}>
                <SearchIcon size={14} />
              </span>
              <input
                className="form-input"
                style={{ paddingLeft: 32 }}
                placeholder="搜索会话与消息…"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
          </div>

          <div style={{ flex: 1, overflowY: "auto", paddingBottom: 8 }}>
            {conversations.length === 0 && (
              <div style={{ padding: "20px 16px", fontSize: 12.5, color: "var(--text-tertiary)", textAlign: "center", lineHeight: 1.7 }}>
                还没有会话
                <br />
                点击上方「新会话」开始
              </div>
            )}
            {conversations.map((c) => (
              <div
                key={c.id}
                className={`conv-item ${c.id === activeId ? "active" : ""}`}
                onClick={() => setActive(c.id)}
                onDoubleClick={(e) => handleRename(e, c.id, c.title)}
                title="双击重命名"
              >
                <span className="conv-title">{c.title || "新会话"}</span>
                <span
                  style={{ fontSize: 11, color: "var(--text-tertiary)", flexShrink: 0 }}
                >
                  {new Date(c.updatedAt).toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" })}
                </span>
                <button className="btn-icon btn-ghost" style={{ width: 24, height: 24 }} onClick={(e) => handleDelete(e, c.id)}>
                  <TrashIcon size={13} />
                </button>
              </div>
            ))}
          </div>
        </>
      )}

      <div className="sidebar-foot">
        每个 Agent 独立隔离
        <br />
        会话 · 授权 · 配置 · 记忆互不共享
      </div>
    </div>
  );
}

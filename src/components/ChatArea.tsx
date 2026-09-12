import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import { useActiveConversation } from "../lib/hooks";
import type { ChatMessage, ToolCallRecord } from "../types";
import { streamChat, type ToolCallChunk } from "../lib/llm";
import { AGENT_TOOLS } from "../lib/agent-tools";
import { listDir, readTextFile, writeTextFile, type AgentId } from "../lib/env";
import { AGENT_META } from "../lib/deepharness";
import { uid, exportConversationJson, exportConversationMarkdown, PERSONAS, notify } from "../lib/storage";
import MessageBubble from "./MessageBubble";
import Composer from "./Composer";
import DeepHarnessView from "./DeepHarnessView";
import LogoMark from "./Logo";
import PersonaMenu from "./PersonaMenu";
import { SparkIcon } from "./Icons";

const SUGGESTIONS = [
  "帮我整理这个文件夹里的文件，按类型分类",
  "写一个 Python 脚本统计当前目录下的文件数量",
  "解释什么是 LLM 的思维链，并举个例子",
  "帮我列一份周末旅行清单",
];

export default function ChatArea() {
  const conv = useActiveConversation();
  const tools = useAppStore((s) => s.tools);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);
  const activeAgent = useAppStore((s) => s.activeAgent);

  const [streaming, setStreaming] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [conv?.messages.length, conv?.messages[conv.messages.length - 1]?.content]);

  const patchAsst = (convId: string, msgId: string, fn: (m: ChatMessage) => ChatMessage) => {
    useAppStore.getState().updateConversation(convId, (c) => ({
      ...c,
      updatedAt: Date.now(),
      messages: c.messages.map((m) => (m.id === msgId ? fn(m) : m)),
    }));
  };

  const send = async (text: string) => {
    const agent: AgentId = useAppStore.getState().activeAgent;
    const st = useAppStore.getState();
    let active = st.activeConversation();
    let convId = active?.id;
    if (!convId) {
      convId = st.newConversation();
      active = useAppStore.getState().activeConversation();
    }
    const model = st.models.find((m) => m.id === active!.modelId) ?? st.models[0];
    if (!model) {
      st.setSettingsOpen(true);
      alert("尚未配置任何模型，请先在设置中添加。");
      return;
    }
    const apiKey = st.settings.apiKeys[model.provider] || st.settings.apiKeys["deepseek"];
    if (!apiKey) {
      st.setSettingsOpen(true);
      alert("请先在设置中填入 API Key，然后选择要用的模型。");
      return;
    }

    const userMsg: ChatMessage = { id: uid(), role: "user", content: text, createdAt: Date.now() };
    const asstMsg: ChatMessage = { id: uid(), role: "assistant", content: "", model: model.name, toolCalls: [], streaming: true, createdAt: Date.now() };

    st.updateConversation(convId, (c) => ({
      ...c,
      updatedAt: Date.now(),
      ...(c.title === "新会话" ? { title: text.slice(0, 22) } : {}),
      messages: [...c.messages, userMsg, asstMsg],
    }));

    setStreaming(true);
    const abort = new AbortController();
    abortRef.current = abort;

    const apiMessages: Record<string, unknown>[] = [];
    const persona = PERSONAS.find((p) => p.id === active!.systemPromptId);
    if (persona?.prompt) apiMessages.push({ role: "system", content: persona.prompt });
    active!.messages
      .filter((m) => m.role === "user" || m.role === "assistant")
      .filter((m) => !m.streaming && !m.error)
      .forEach((m) => apiMessages.push({ role: m.role, content: m.content }));
    apiMessages.push({ role: "user", content: text });

    const withTools = model.supportsTools && tools.fileTools;

    try {
      for (let round = 0; round < 6; round++) {
        let toolCalls: ToolCallChunk[] = [];
        await streamChat({
          baseUrl: model.baseUrl,
          apiKey,
          model: model.model,
          messages: apiMessages as { role: string; content: string }[],
          temperature: model.temperature,
          maxTokens: model.maxTokens,
          tools: withTools ? AGENT_TOOLS : undefined,
          signal: abort.signal,
          onDelta: (d) => {
            patchAsst(convId, asstMsg.id, (m) => ({ ...m, content: m.content + d }));
          },
          onToolCallStart: (calls) => {
            toolCalls = calls;
          },
        });

        if (!toolCalls.length) break;

        const records: ToolCallRecord[] = [];
        for (let i = 0; i < toolCalls.length; i++) {
          const tc = toolCalls[i];
          if (!tc) continue;
          let result = "";
          let ok = true;
          try {
            const args = JSON.parse(tc.args || "{}");
            if (tc.name === "list_dir") {
              const r = await listDir(agent, String(args.path || "."));
              result = r.map((f) => `${f.isDir ? "[dir] " : ""}${f.name}${f.isDir ? "" : ` (${f.size} B)`}`).join("\n");
            } else if (tc.name === "read_file") {
              result = await readTextFile(agent, String(args.path || ""));
            } else if (tc.name === "write_file") {
              const written = await writeTextFile(
                agent,
                String(args.path || ""),
                typeof args.content === "string" ? args.content : String(args.content ?? "")
              );
              result = `已写入 ${written} 字节`;
            } else {
              result = `未知工具: ${tc.name}`;
              ok = false;
            }
          } catch (e) {
            ok = false;
            const msg = e instanceof Error ? e.message : String(e);
            if (msg.includes("路径不在授权范围内")) {
              result =
                "该路径未被授权。请告知用户：在界面上选择并授权该文件/文件夹后重试（每个 Agent 的授权相互独立）。";
            } else {
              result = msg;
            }
          }
          records.push({ name: tc.name || "tool", args: tc.args || "", result, ok });
        }

        patchAsst(convId, asstMsg.id, (m) => ({ ...m, toolCalls: records }));

        apiMessages.push({
          role: "assistant",
          content: "",
          tool_calls: toolCalls.map((tc, i) => ({
            id: tc.id || `call_${round}_${i}`,
            type: "function",
            function: { name: tc.name, arguments: tc.args || "{}" },
          })),
        });
        toolCalls.forEach((tc, i) => {
          apiMessages.push({
            role: "tool",
            tool_call_id: tc.id || `call_${round}_${i}`,
            content: records[i]?.result || "",
          });
        });
      }

      patchAsst(convId, asstMsg.id, (m) => ({ ...m, streaming: false }));
      notify("DeepHarness", "已生成回复");
    } catch (e) {
      if (!abort.signal.aborted) {
        const errMsg = e instanceof Error ? e.message : String(e);
        patchAsst(convId, asstMsg.id, (m) => ({
          ...m,
          streaming: false,
          error: true,
          content: m.content || `请求失败：${errMsg}`,
        }));
      } else {
        patchAsst(convId, asstMsg.id, (m) => ({ ...m, streaming: false }));
      }
    } finally {
      setStreaming(false);
      abortRef.current = null;
    }
  };

  const stop = () => abortRef.current?.abort();

  // 任务型 Agent 不走对话形态：整块主视图交给任务控制台。
  if (AGENT_META[activeAgent].hasTaskConsole) {
    return <DeepHarnessView />;
  }

  const empty = !conv || conv.messages.length === 0;

  return (
    <div className="main">
      {conv && conv.messages.length > 0 && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "10px 8%",
            borderBottom: "1px solid var(--border)",
            background: "var(--surface)",
          }}
        >
          <span style={{ fontSize: 13.5, fontWeight: 500, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {conv.title || "新会话"}
          </span>
          <PersonaMenu />
          <button className="btn btn-sm btn-ghost" onClick={() => exportConversationMarkdown(conv)}>导出 MD</button>
          <button className="btn btn-sm btn-ghost" onClick={() => exportConversationJson(conv)}>导出 JSON</button>
        </div>
      )}

      {empty ? (
        <div className="empty-state">
          <div className="logo-float"><LogoMark size={64} radius={18} /></div>
          <div>
            <div style={{ fontSize: 18, fontWeight: 600, color: "var(--text)" }}>开始一段新的对话</div>
            <div style={{ fontSize: 13, marginTop: 6 }}>
              {AGENT_META[activeAgent].name} · {AGENT_META[activeAgent].tagline}
            </div>
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10, maxWidth: 500, width: "100%" }}>
            {SUGGESTIONS.map((s) => (
              <button key={s} className="btn suggestion" onClick={() => send(s)}>
                {s}
              </button>
            ))}
          </div>
          <button className="btn" style={{ marginTop: 4 }} onClick={() => setSettingsOpen(true)}>
            <SparkIcon size={14} /> 配置 API Key 与模型
          </button>
        </div>
      ) : (
        <div className="msg-list" ref={listRef}>
          {conv!.messages.map((m) => (
            <MessageBubble key={m.id} msg={m} />
          ))}
        </div>
      )}

      <Composer onSend={send} onStop={stop} streaming={streaming} canSend={!streaming} />
    </div>
  );
}

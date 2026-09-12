import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import type { ChatMessage, ToolCallRecord } from "../types";
import { streamChat, type ToolCallChunk } from "../lib/llm";
import { listDir, runTool } from "../lib/env";
import { uid, exportConversationJson, exportConversationMarkdown, PERSONAS, notify } from "../lib/storage";
import MessageBubble from "./MessageBubble";
import Composer from "./Composer";
import LogoMark from "./Logo";
import PersonaMenu from "./PersonaMenu";
import { SparkIcon } from "./Icons";

const AGENT_TOOLS = [
  {
    type: "function",
    function: {
      name: "run_command",
      description: "在用户电脑上执行命令或运行脚本（Python / Node / 系统命令），返回标准输出与错误输出。cwd 为工作目录。",
      parameters: {
        type: "object",
        properties: {
          command: { type: "string", description: "要执行的命令" },
          args: { type: "array", items: { type: "string" }, description: "命令参数列表" },
          cwd: { type: "string", description: "工作目录，可省略" },
        },
        required: ["command"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "list_dir",
      description: "列出指定目录下的文件与子目录，用于了解文件结构。",
      parameters: {
        type: "object",
        properties: {
          path: { type: "string", description: "目录路径" },
        },
        required: ["path"],
      },
    },
  },
];

const SUGGESTIONS = [
  "帮我整理这个文件夹里的文件，按类型分类",
  "写一个 Python 脚本统计当前目录下的文件数量",
  "解释什么是 LLM 的思维链，并举个例子",
  "帮我列一份周末旅行清单",
];

export default function ChatArea() {
  const conv = useAppStore((s) => s.activeConversation());
  const tools = useAppStore((s) => s.tools);
  const setSettingsOpen = useAppStore((s) => s.setSettingsOpen);

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

    const withTools = model.supportsTools && (tools.code || tools.file || tools.search);

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
            if (tc.name === "run_command") {
              const r = await runTool(String(args.command || ""), Array.isArray(args.args) ? args.args : [], args.cwd ? String(args.cwd) : undefined);
              result = `exit=${r.code}\n${r.stdout || ""}${r.stderr ? `\n[stderr]\n${r.stderr}` : ""}`;
            } else if (tc.name === "list_dir") {
              const r = await listDir(String(args.path || "."));
              result = r.map((f) => `${f.isDir ? "[dir] " : ""}${f.name}${f.isDir ? "" : ` (${f.size} B)`}`).join("\n");
            } else {
              result = `未知工具: ${tc.name}`;
              ok = false;
            }
          } catch (e) {
            ok = false;
            result = String(e);
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
            <div style={{ fontSize: 13, marginTop: 6 }}>星核 StarCore · 选择模型后即可开始</div>
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

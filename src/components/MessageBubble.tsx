import { lazy, Suspense } from "react";
import type { ChatMessage } from "../types";
import LogoMark from "./Logo";
import { UserIcon } from "./Icons";

// marked + highlight.js（含全部语言）约 1MB，只有真正要渲染消息时才需要。
// 静态引入会把它塞进首个 chunk：安装包体积不变，但首帧要多解析/求值 1MB JS。
// 拆成异步块后首屏只加载 React 运行时与业务代码，代码高亮按需到达。
// 挂载前先预热一次，使首条消息大概率无需等待。
const Markdown = lazy(() => import("./Markdown"));
// 应用启动即开始下载该异步块（本组件在启动路径上被静态引入）。
// 下载与首帧并行进行，故首条消息渲染时通常已就绪，Suspense 不会可见地闪一下。
void import("./Markdown");

export default function MessageBubble({ msg }: { msg: ChatMessage }) {
  if (msg.role === "user") {
    return (
      <div className="msg-row user">
        <div className="bubble user" style={{ whiteSpace: "pre-wrap" }}>{msg.content}</div>
        <div className="avatar user"><UserIcon size={16} /></div>
      </div>
    );
  }

  const showTyping = msg.streaming && !msg.content;

  return (
    <div className="msg-row">
      <div className="avatar ai"><LogoMark size={28} radius={8} /></div>
      <div style={{ flex: 1, minWidth: 0 }}>
        {msg.toolCalls && msg.toolCalls.length > 0 && (
          <div>
            {msg.toolCalls.map((tc, i) => (
              <div className="tool-call-box" key={i}>
                <span className="tool-name">{tc.name}</span>
                {tc.ok ? (
                  <span style={{ marginLeft: 8, color: "var(--success)", fontSize: 12 }}>执行成功</span>
                ) : (
                  <span style={{ marginLeft: 8, color: "var(--danger)", fontSize: 12 }}>执行失败</span>
                )}
                <pre>{tc.result.slice(0, 600)}{tc.result.length > 600 ? " …" : ""}</pre>
              </div>
            ))}
          </div>
        )}
        <div className={`bubble ai ${msg.error ? "error" : ""}`}>
          {showTyping ? (
            <span className="typing-dots"><span /><span /><span /></span>
          ) : msg.content ? (
            <Suspense
              fallback={
                // 异步块到位前先按纯文本显示，避免气泡空白或高度跳变。
                <div style={{ whiteSpace: "pre-wrap" }}>{msg.content}</div>
              }
            >
              <Markdown content={msg.content} />
            </Suspense>
          ) : (
            <span style={{ color: "var(--text-tertiary)" }}>（无内容）</span>
          )}
          {msg.streaming && msg.content && (
            <span className="typing-dots" style={{ display: "inline-flex", marginLeft: 4 }}><span /><span /><span /></span>
          )}
        </div>
        {msg.model && (
          <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 4, paddingLeft: 2 }}>{msg.model}</div>
        )}
      </div>
    </div>
  );
}

import { useEffect, useMemo, useRef } from "react";
import { marked } from "marked";
import { highlightCodeBlocks } from "../lib/highlight";
// 主题样式随本组件一起进异步块：只有真正要渲染消息时才加载，
// 不占用首屏 CSS。语言定义在 lib/highlight 里按需注册。
import "highlight.js/styles/atom-one-dark.css";

marked.setOptions({ gfm: true, breaks: true });

export default function Markdown({ content }: { content: string }) {
  const ref = useRef<HTMLDivElement>(null);

  const html = useMemo(() => {
    try {
      return marked.parse(content) as string;
    } catch {
      // 解析失败（例如流式传输中断在半截语法上）时退回纯文本，不丢内容。
      return content;
    }
  }, [content]);

  useEffect(() => {
    if (ref.current) highlightCodeBlocks(ref.current);
  }, [html]);

  return <div className="markdown-body" ref={ref} dangerouslySetInnerHTML={{ __html: html }} />;
}

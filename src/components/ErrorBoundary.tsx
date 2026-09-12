import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
  componentStack: string;
  copied: boolean;
}

/**
 * 顶层错误边界。
 *
 * 存在的意义：React 在渲染阶段抛异常时会卸载整棵组件树，`#root` 变空，
 * 用户看到的只有 `body` 的纯色背景——也就是一个「什么都没有的黑屏」，
 * 既无报错也无出路。本组件把异常留在界面里，给出可读的错误详情、
 * 一键复制与重新加载，保证任何情况下界面都有出路。
 *
 * 注意：错误边界只能捕获**渲染期**异常（含子组件生命周期）。
 * 事件回调、`setTimeout`、Promise 里的异常不会走到这里，那些地方仍需
 * 各自的 try/catch。
 */
export default class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null, componentStack: "", copied: false };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    // 开发者控制台同时留一份，便于带 devtools 的调试构建定位。
    console.error("[DeepHarness] 界面渲染异常：", error, info.componentStack);
    this.setState({ componentStack: info.componentStack ?? "" });
  }

  private detailText(): string {
    const { error, componentStack } = this.state;
    return [
      "DeepHarness 界面渲染异常",
      `时间：${new Date().toLocaleString("zh-CN")}`,
      `页面：${typeof location !== "undefined" ? location.href : "-"}`,
      `环境：${typeof navigator !== "undefined" ? navigator.userAgent : "-"}`,
      "",
      `${error?.name ?? "Error"}: ${error?.message ?? "-"}`,
      componentStack ? `\n组件栈：${componentStack}` : "",
    ].join("\n");
  }

  private reset = (): void => {
    this.setState({ error: null, componentStack: "", copied: false });
  };

  private copy = (): void => {
    void (async () => {
      try {
        await navigator.clipboard.writeText(this.detailText());
        this.setState({ copied: true });
      } catch {
        // 剪贴板被拒时不清空提示；详情本身就在界面上，用户可手动选中复制。
        this.setState({ copied: false });
      }
    })();
  };

  override render(): ReactNode {
    const { error, componentStack, copied } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="modal-mask" style={{ zIndex: 9999 }}>
        <div className="modal" style={{ width: 620 }}>
          <div className="modal-head">
            <span style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 15, fontWeight: 600 }}>
              <span
                aria-hidden="true"
                style={{
                  width: 9,
                  height: 9,
                  borderRadius: "50%",
                  background: "var(--danger)",
                  boxShadow: "0 0 8px var(--danger)",
                  flexShrink: 0,
                }}
              />
              界面渲染出错
            </span>
            <button className="btn btn-sm" onClick={this.reset}>
              重试
            </button>
          </div>

          <div className="modal-body">
            <div style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.75, marginBottom: 14 }}>
              界面组件在渲染时抛出了异常，DeepHarness 已把影响范围隔离在这里。
              你的会话与配置都存在本机，没有丢失；可以先点「重试」，
              若反复出现请把下面的详情反馈给作者。
            </div>

            <pre
              style={{
                margin: 0,
                padding: "12px 14px",
                background: "var(--bg-secondary)",
                border: "1px solid var(--border)",
                borderRadius: "var(--radius-sm)",
                fontSize: 12,
                lineHeight: 1.65,
                color: "var(--text)",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                maxHeight: 200,
                overflowY: "auto",
              }}
            >
              {error.name}: {error.message}
            </pre>

            {componentStack && (
              <details style={{ marginTop: 12 }}>
                <summary
                  style={{ cursor: "pointer", fontSize: 12.5, color: "var(--text-secondary)", userSelect: "none" }}
                >
                  查看组件调用栈
                </summary>
                <pre
                  style={{
                    margin: "10px 0 0",
                    padding: "12px 14px",
                    background: "var(--bg-secondary)",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius-sm)",
                    fontSize: 11.5,
                    lineHeight: 1.6,
                    color: "var(--text-secondary)",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                    maxHeight: 200,
                    overflowY: "auto",
                  }}
                >
                  {componentStack.trim()}
                </pre>
              </details>
            )}
          </div>

          <div className="modal-foot">
            <button className="btn" onClick={this.copy} disabled={copied}>
              {copied ? "已复制" : "复制错误详情"}
            </button>
            <button className="btn btn-primary" onClick={() => window.location.reload()}>
              重新加载界面
            </button>
          </div>
        </div>
      </div>
    );
  }
}

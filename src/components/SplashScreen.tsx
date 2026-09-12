import LogoMark from "./Logo";

interface Props {
  leaving?: boolean;
  status?: string;
  error?: string | null;
  onRetry?: () => void;
}

// 启动动画页：深空星野 + 鲸鱼标志辉光 + 环形轨道 + 扫描进度
export default function SplashScreen({ leaving, status, error, onRetry }: Props) {
  return (
    <div className={leaving ? "splash splash-leaving" : "splash"}>
      <div className="splash-stars" aria-hidden="true" />
      <div className="splash-core">
        <div className="splash-orbit">
          <div className="splash-orbit-ring" />
          <div className="splash-orbit-dot" />
          <div className="splash-logo">
            <LogoMark size={86} radius={22} />
          </div>
        </div>
        <div className="splash-brand">
          <span className="splash-title">DeepHarness</span>
          <span className="splash-sub">零配置的 AI Agent 桌面工作台</span>
        </div>
        <div className="splash-status">
          {error ? (
            <span className="splash-error">{error}</span>
          ) : (
            <>
              <span>{status ?? "正在初始化"}</span>
              <span className="splash-dots">
                <i />
                <i />
                <i />
              </span>
            </>
          )}
        </div>
        {error && onRetry && (
          <button className="btn" onClick={onRetry}>
            重试
          </button>
        )}
      </div>
      <div className="splash-bar">
        <div className="splash-bar-fill" />
      </div>
    </div>
  );
}

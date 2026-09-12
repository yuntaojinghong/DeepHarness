import { useState, type ReactNode } from "react";
import { useAppStore } from "../store";
import LogoMark from "./Logo";
import {
  CheckIcon,
  CloseIcon,
  CoffeeIcon,
  ExternalLinkIcon,
  HeartIcon,
  StarIcon,
} from "./Icons";
import {
  AUTHOR_NAME,
  ISSUES_URL,
  REPO_URL,
  SUPPORT_QR_CODE,
  openExternalUrlSafe,
} from "../lib/links";

/** 支持方式卡片：一致的排版，避免两处各写一套 inline style。 */
function SupportCard({
  icon,
  title,
  desc,
  action,
}: {
  icon: ReactNode;
  title: string;
  desc: string;
  action: ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        gap: 14,
        padding: "14px 16px",
        borderRadius: 12,
        // 使用项目主题变量（--border / --bg-secondary），保证明暗两套
        // 主题下都有正确的层次与对比度。
        border: "1px solid var(--border)",
        background: "var(--bg-secondary)",
      }}
    >
      <div
        style={{
          width: 34,
          height: 34,
          flexShrink: 0,
          display: "grid",
          placeItems: "center",
          borderRadius: 9,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          color: "var(--text-secondary)",
        }}
      >
        {icon}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 3 }}>{title}</div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-secondary)",
            lineHeight: 1.65,
            marginBottom: 10,
          }}
        >
          {desc}
        </div>
        {action}
      </div>
    </div>
  );
}

export default function SupportModal() {
  const setSupportOpen = useAppStore((s) => s.setSupportOpen);
  const [starState, setStarState] = useState<"idle" | "done" | "error">("idle");
  const [error, setError] = useState<string | null>(null);

  const goStar = async () => {
    setError(null);
    const message = await openExternalUrlSafe(REPO_URL);
    if (message) {
      setStarState("error");
      setError(message);
      return;
    }
    setStarState("done");
  };

  const goIssues = async () => {
    setError(null);
    const message = await openExternalUrlSafe(ISSUES_URL);
    if (message) setError(message);
  };

  return (
    <div className="modal-mask">
      <div className="modal" style={{ width: 560 }}>
        <div className="modal-head">
          <span style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 15, fontWeight: 600 }}>
            <HeartIcon size={18} /> 支持 DeepHarness
          </span>
          <button
            className="btn-icon btn-ghost"
            onClick={() => setSupportOpen(false)}
            title="关闭"
          >
            <CloseIcon />
          </button>
        </div>

        <div className="modal-body">
          <div style={{ fontSize: 13, color: "var(--text-secondary)", lineHeight: 1.7, marginBottom: 18 }}>
            DeepHarness 由 {AUTHOR_NAME} 独立开发维护，完全本地运行、不上传任何数据。
            如果它帮你省下了时间，下面两种方式都是很好的支持。
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <SupportCard
              icon={<StarIcon size={17} />}
              title="在 GitHub 点个 Star"
              desc="最省事的支持。Star 能让更多人发现这个项目，也方便你跟进更新。"
              action={
                <button className="btn btn-primary" onClick={goStar}>
                  {starState === "done" ? <CheckIcon size={14} /> : <StarIcon size={14} />}
                  {starState === "done" ? "已在浏览器中打开" : "去点 Star"}
                  {starState !== "done" && <ExternalLinkIcon size={12} />}
                </button>
              }
            />

            <SupportCard
              icon={<CoffeeIcon size={17} />}
              title="请作者喝杯咖啡"
              desc="扫码赞赏，金额随意。打赏不会解锁额外功能 —— 所有能力本来就是全开的。"
              action={
                <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                  <img
                    src={SUPPORT_QR_CODE}
                    alt="微信赞赏码"
                    width={168}
                    height={168}
                    style={{
                      width: 168,
                      height: 168,
                      objectFit: "contain",
                      borderRadius: 10,
                      // 赞赏码本体是白底黑模块，加一圈浅边框让它在深色
                      // 主题下不显得突兀（底色不能用主题变量，否则会压掉
                      // 二维码的对比度导致识别失败）。
                      background: "#ffffff",
                      padding: 8,
                      boxSizing: "content-box",
                      border: "1px solid var(--border)",
                    }}
                  />
                  <div style={{ fontSize: 12, color: "var(--text-tertiary)", lineHeight: 1.7 }}>
                    微信扫一扫
                    <br />
                    即可赞赏
                  </div>
                </div>
              }
            />
          </div>

          {error && (
            <div
              style={{
                marginTop: 14,
                padding: "9px 12px",
                borderRadius: 9,
                fontSize: 12,
                lineHeight: 1.6,
                color: "var(--text-secondary)",
                background: "rgba(200,80,80,0.10)",
                border: "1px solid rgba(200,80,80,0.30)",
              }}
            >
              打开链接失败：{error}
            </div>
          )}
        </div>

        <div className="modal-foot" style={{ justifyContent: "space-between" }}>
          <button className="btn btn-ghost" onClick={goIssues} style={{ fontSize: 12 }}>
            遇到问题？反馈一下
          </button>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ fontSize: 11, color: "var(--text-tertiary)", display: "flex", alignItems: "center", gap: 6 }}>
              <LogoMark size={16} radius={5} /> 感谢每一位支持者
            </span>
            <button className="btn" onClick={() => setSupportOpen(false)}>
              关闭
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

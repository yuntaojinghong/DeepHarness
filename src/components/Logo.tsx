import { useId } from "react";

interface Props {
  size?: number;
  radius?: number;
}

/**
 * DeepHarness 品牌标志：开口挽具环 + 中心核心 + 三个 Agent 节点。
 * 石墨深空底，钢蓝 → 青瓷单一渐变，深浅主题下均可直接使用。
 */
export default function LogoMark({ size = 28, radius = 24 }: Props) {
  const id = useId().replace(/:/g, "");
  const bgId = `dh-bg-${id}`;
  const ringId = `dh-ring-${id}`;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 512 512"
      fill="none"
      style={{ display: "block", flexShrink: 0 }}
    >
      <defs>
        <linearGradient id={bgId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#242B3A" />
          <stop offset="1" stopColor="#151A24" />
        </linearGradient>
        <linearGradient id={ringId} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#6E9BFF" />
          <stop offset="1" stopColor="#63C7BE" />
        </linearGradient>
      </defs>

      {/* 底板圆角方（radius 参数按 512 视图等比缩放） */}
      <rect
        x="16"
        y="16"
        width="480"
        height="480"
        rx={Math.round(112 * (radius / 112))}
        fill={`url(#${bgId})`}
      />

      {/* 挽具环（顶部 60° 开口） */}
      <path
        d="M 320 155.2 A 128 128 0 0 1 256 394 A 128 128 0 0 1 192 155.2"
        fill="none"
        stroke={`url(#${ringId})`}
        strokeWidth="40"
        strokeLinecap="round"
      />

      {/* 中心核心 */}
      <circle cx="256" cy="266" r="34" fill="#F4F7FB" />

      {/* 顶部引导节点 */}
      <circle cx="256" cy="138" r="18" fill={`url(#${ringId})`} />

      {/* 环上两个 Agent 节点 */}
      <circle cx="320" cy="376.8" r="12" fill="#DCE4F0" />
      <circle cx="192" cy="376.8" r="12" fill="#DCE4F0" />
    </svg>
  );
}

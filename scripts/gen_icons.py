# -*- coding: utf-8 -*-
"""
DeepHarness 图标生成器。

从零用 Pillow 渲染品牌几何（与 app-icon.svg 完全同构），
输出 src-tauri/icons/ 下 Tauri 打包所需的全部位图与 ICO：

  icon.png / 128x128.png / 128x128@2x.png / 32x32.png / 64x64.png
  icon.ico（256/128/64/48/32/16 多尺寸）
  Square*/StoreLogo.png（Windows 磁贴）

用法：python scripts/gen_icons.py
依赖：pip install pillow
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps

# ---- 品牌几何常量（512 视图，与 app-icon.svg 一致） ----
CANVAS = 512
TILE_INSET = 16
TILE_RADIUS = 112
BG_TOP = (0x24, 0x2B, 0x3A)
BG_BOTTOM = (0x15, 0x1A, 0x24)
RING_BLUE = (0x6E, 0x9B, 0xFF)
RING_TEAL = (0x63, 0xC7, 0xBE)
RING_WIDTH = 40
RING_RADIUS = 128
CORE_CENTER = (256.0, 266.0)
CORE_RADIUS = 34
CAP_WHITE = (0xF4, 0xF7, 0xFB)
NODE_SOFT = (0xDC, 0xE4, 0xF0)

ICONS_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"

# 环的角度划分（Pillow arc：0° = 3 点钟方向，顺时针，y 轴向下）
GAP_DEG = 60  # 顶部开口角度
TOP_DEG = 270  # 12 点钟方向
ARC_END_RIGHT = (TOP_DEG + GAP_DEG / 2) % 360  # 300°，开口右缘
ARC_END_LEFT = (TOP_DEG - GAP_DEG / 2) % 360  # 240°，开口左缘
BOTTOM_DEG = 90  # 6 点钟方向，左右两段的分界


def _gradient_tile(size: int) -> Image.Image:
    """生成垂直渐变的圆角方底板（RGBA，边缘透明）。"""
    gradient = Image.linear_gradient("L").resize((size, size), Image.LANCZOS)
    tile = ImageOps.colorize(
        gradient, black=BG_TOP, white=BG_BOTTOM
    ).convert("RGBA")

    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (
            round(TILE_INSET * size / CANVAS),
            round(TILE_INSET * size / CANVAS),
            round((CANVAS - TILE_INSET) * size / CANVAS),
            round((CANVAS - TILE_INSET) * size / CANVAS),
        ),
        radius=round(TILE_RADIUS * size / CANVAS),
        fill=255,
    )

    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(tile, (0, 0), mask)
    return out


def _ring_point(deg: float, scale: float) -> tuple[float, float]:
    """环上某角度的圆心坐标（512 视图坐标，y 轴向下、顺时针）。"""
    rad = math.radians(deg)
    x = CORE_CENTER[0] + RING_RADIUS * math.cos(rad)
    y = CORE_CENTER[1] + RING_RADIUS * math.sin(rad)
    return (x * scale, y * scale)


def _draw_ring(draw: ImageDraw.ImageDraw, scale: float) -> None:
    """绘制挽具环：右段青瓷、左段钢蓝，端点补圆头。"""
    bbox = (
        (CORE_CENTER[0] - RING_RADIUS - RING_WIDTH / 2) * scale,
        (CORE_CENTER[1] - RING_RADIUS - RING_WIDTH / 2) * scale,
        (CORE_CENTER[0] + RING_RADIUS + RING_WIDTH / 2) * scale,
        (CORE_CENTER[1] + RING_RADIUS + RING_WIDTH / 2) * scale,
    )
    stroke = RING_WIDTH * scale

    # 右段：开口右缘 300° → 底部 90°（顺时针）
    draw.arc(bbox, start=ARC_END_RIGHT, end=BOTTOM_DEG, fill=RING_TEAL, width=round(stroke))
    # 左段：底部 90° → 开口左缘 240°（顺时针）
    draw.arc(bbox, start=BOTTOM_DEG, end=ARC_END_LEFT, fill=RING_BLUE, width=round(stroke))

    # 三处圆头端点（300° / 90° / 240°）
    for deg in (ARC_END_RIGHT, BOTTOM_DEG, ARC_END_LEFT):
        cx, cy = _ring_point(deg, scale)
        r = stroke / 2
        draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=_seg_color(deg))


def _seg_color(deg: float) -> tuple[int, int, int]:
    """端点圆头取所属段的颜色：底部端点右属青瓷、左属钢蓝。"""
    if deg == BOTTOM_DEG:
        # 底部端点被两段共享，取中点色以平滑衔接
        return tuple((a + b) // 2 for a, b in zip(RING_BLUE, RING_TEAL))  # type: ignore[return-value]
    return RING_TEAL if deg == ARC_END_RIGHT else RING_BLUE


def _draw_gradient_circle(
    base: Image.Image, center: tuple[float, float], radius: float,
    top_color: tuple[int, int, int], bottom_color: tuple[int, int, int],
) -> None:
    """在 base 上绘制一个垂直渐变填充的圆（用于顶部引导节点）。"""
    size = max(2, round(radius * 2))
    gradient = Image.linear_gradient("L").resize((size, size), Image.LANCZOS)
    fill = ImageOps.colorize(gradient, black=top_color, white=bottom_color)

    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, size - 1, size - 1), fill=255)

    cx, cy = center
    base.paste(fill, (round(cx - radius), round(cy - radius)), mask)


def render_master() -> Image.Image:
    """渲染 512px 主图标。"""
    img = _gradient_tile(CANVAS)
    draw = ImageDraw.Draw(img)
    scale = 1.0

    _draw_ring(draw, scale)

    # 环上左下 / 右下两个 Agent 节点（60° / 120°）
    for deg in (60, 120):
        cx, cy = _ring_point(deg, scale)
        r = 12 * scale
        draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=NODE_SOFT)

    # 中心核心
    cx, cy = CORE_CENTER
    r = CORE_RADIUS * scale
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=CAP_WHITE)

    # 顶部开口处的引导节点（渐变）
    _draw_gradient_circle(img, _ring_point(TOP_DEG, scale), 18 * scale, RING_BLUE, RING_TEAL)

    return img


def render_at(size: int) -> Image.Image:
    """渲染任意尺寸（先渲染 1024 超采样再缩小，保证小尺寸边缘平滑）。"""
    master = render_master().resize((size * 2, size * 2), Image.LANCZOS)
    return master.resize((size, size), Image.LANCZOS)


def main() -> None:
    ICONS_DIR.mkdir(parents=True, exist_ok=True)

    master = render_master()

    outputs: dict[str, int] = {
        "icon.png": 512,
        "128x128@2x.png": 256,
        "128x128.png": 128,
        "64x64.png": 64,
        "32x32.png": 32,
        # Windows 磁贴资源
        "Square30x30Logo.png": 30,
        "Square44x44Logo.png": 44,
        "Square71x71Logo.png": 71,
        "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107,
        "Square142x142Logo.png": 142,
        "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284,
        "Square310x310Logo.png": 310,
        "StoreLogo.png": 50,
    }
    for name, size in outputs.items():
        render_at(size).save(ICONS_DIR / name)
        print(f"[ok] {name} ({size}x{size})")

    # 多尺寸 ICO（Windows 应用图标 + 安装程序图标）
    ico_sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)]
    master.save(
        ICONS_DIR / "icon.ico",
        format="ICO",
        sizes=ico_sizes,
        append_images=[render_at(s[0]) for s in ico_sizes[1:]],
    )
    print("[ok] icon.ico " + "/".join(f"{w}" for w, _ in ico_sizes))

    print(f"\n全部图标已输出到 {ICONS_DIR}")


if __name__ == "__main__":
    main()

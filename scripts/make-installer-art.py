#!/usr/bin/env python3
"""生成 NSIS 安装程序的品牌素材（header 150x57 / sidebar 164x314 BMP）。

为什么需要这个脚本
------------------
Tauri 的 NSIS 打包会直接使用 `installer-header.bmp` 与 `installer-sidebar.bmp`。
仓库里原先躺着的是**上一个项目（StarCore）的旧素材**：鲸鱼图形、
「DeepSeek Harness」字样、「v0.3.0」与「STARCORE」落款。
它与本项目（DeepHarness、挽具环标志）毫无关系，用户装完看到的就是别人的品牌页。

因此这里按 `src/components/Logo.tsx` 的真实品牌几何重绘，保证：
  - 图形、渐变、配色与界面里的标志完全一致；
  - **刻意不在图里写版本号**。旧素材的「v0.3.0」正是硬编码版本留下的债 ——
    版本升一次就永久失真。NSIS 自己的界面会显示真实版本，此处不必重复。

用法
----
    python scripts/make-installer-art.py [--out src-tauri/icons]

依赖 Pillow。渲染采用 4 倍超采样后降采样，以得到干净的边缘。
"""

from __future__ import annotations

import argparse
import math
import os
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:  # pragma: no cover - 环境缺依赖时给出可执行的提示
    sys.exit("需要 Pillow：python -m pip install pillow")

# ---- 品牌常量（与 Logo.tsx / global.css 保持一致） ----
BG_TOP = (36, 43, 58)       # #242B3A
BG_BOTTOM = (21, 26, 36)    # #151A24
RING_FROM = (110, 155, 255)  # #6E9BFF
RING_TO = (99, 199, 190)     # #63C7BE
CORE = (244, 247, 251)       # #F4F7FB
NODE = (220, 228, 240)       # #DCE4F0
TEXT = (244, 247, 251)
TEXT_DIM = (166, 174, 196)   # #A6AEC4
TEXT_FAINT = (110, 120, 145)

SS = 4  # 超采样倍数

# NSIS 标准尺寸
HEADER_SIZE = (150, 57)
SIDEBAR_SIZE = (164, 314)


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def load_font(paths, size):
    """按顺序取第一个可用的字体，避免在缺字体时直接崩掉。"""
    for p in paths:
        if os.path.isfile(p):
            try:
                return ImageFont.truetype(p, size)
            except OSError:
                continue
    return ImageFont.load_default()


def text_width(draw, text, font):
    box = draw.textbbox((0, 0), text, font=font)
    return box[2] - box[0]


def fit_font(draw, text, paths, size, max_w):
    """字号从 size 起逐级缩小，直到文字宽度放得下。"""
    while size > 6:
        f = load_font(paths, size)
        if text_width(draw, text, f) <= max_w:
            return f
        size -= 1
    return load_font(paths, size)


def draw_logo(img, box):
    """在 box=(x, y, size) 区域内绘制品牌标志（圆角底板 + 挽具环 + 核心 + 三节点）。

    几何完全照搬 Logo.tsx 的 512 viewBox：环心 (256,266)、半径 128、
    描边 40，顶部 60° 开口（即顺时针 300° 弧）。
    """
    x, y, size = box
    s = size / 512.0
    d = ImageDraw.Draw(img)

    # 圆角底板
    d.rounded_rectangle(
        [x, y, x + size, y + size],
        radius=112 * s,
        fill=None,
        outline=None,
    )
    # 底板用逐行渐变填充
    tile = Image.new("RGB", (size, size))
    td = ImageDraw.Draw(tile)
    for i in range(size):
        td.line([(0, i), (size, i)], fill=lerp(BG_TOP, BG_BOTTOM, i / max(size - 1, 1)))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size - 1, size - 1], radius=112 * s, fill=255)
    img.paste(tile, (x, y), mask)

    cx, cy, r = x + 256 * s, y + 266 * s, 128 * s
    w = 40 * s

    # 顺时针 300° 弧，顶部留 60° 开口。逐段取色以模拟 SVG 的线性渐变。
    start, end = 300.0, 600.0
    steps = max(int((end - start) / 2), 24)
    prev = None
    for i in range(steps + 1):
        a = math.radians(start + (end - start) * i / steps)
        px, py = cx + r * math.cos(a), cy + r * math.sin(a)
        t = ((px - x) / size + (py - y) / size) / 2.0
        color = lerp(RING_FROM, RING_TO, max(0.0, min(1.0, t)))
        if prev is not None:
            d.line([prev, (px, py)], fill=color, width=round(w))
        # 圆头：在每一段端点补圆，避免转折处出现缺口
        d.ellipse([px - w / 2, py - w / 2, px + w / 2, py + w / 2], fill=color)
        prev = (px, py)

    # 中心核心
    core_r = 34 * s
    d.ellipse([cx - core_r, cy - core_r, cx + core_r, cy + core_r], fill=CORE)

    # 顶部引导节点
    nd = 18 * s
    d.ellipse([cx - nd, cy - nd, cx + nd, cy + nd], fill=RING_TO)

    # 环上两个 Agent 节点
    nr = 12 * s
    for ang in (60.0, 120.0):
        a = math.radians(ang)
        nx, ny = cx + r * math.cos(a), cy + r * math.sin(a)
        d.ellipse([nx - nr, ny - nr, nx + nr, ny + nr], fill=NODE)


def vertical_gradient(size):
    img = Image.new("RGB", size)
    d = ImageDraw.Draw(img)
    h = size[1]
    for i in range(h):
        d.line([(0, i), (size[0], i)], fill=lerp(BG_TOP, BG_BOTTOM, i / max(h - 1, 1)))
    return img


def make_header(out_path, cjk_paths, latin_paths):
    """150x57 顶部条：标志 + 产品名 + 一句话定位。"""
    W, H = HEADER_SIZE
    img = vertical_gradient((W * SS, H * SS))
    d = ImageDraw.Draw(img)

    logo = 38 * SS
    draw_logo(img, (12 * SS, (H * SS - logo) // 2, logo))

    tx = 12 * SS + logo + 10 * SS
    max_w = W * SS - tx - 8 * SS
    f_title = fit_font(d, "DeepHarness", latin_paths, 15 * SS, max_w)
    f_sub = fit_font(d, "零配置 AI Agent 工作台", cjk_paths, 8 * SS, max_w)

    d.text((tx, 15 * SS), "DeepHarness", font=f_title, fill=TEXT)
    d.text((tx, 33 * SS), "零配置 AI Agent 工作台", font=f_sub, fill=TEXT_DIM)

    img.resize((W, H), Image.LANCZOS).save(out_path, "BMP")
    return out_path


def make_sidebar(out_path, cjk_paths, latin_paths):
    """164x314 侧栏：标志 + 产品名 + 定位 + 三条卖点。刻意不含版本号。"""
    W, H = SIDEBAR_SIZE
    img = vertical_gradient((W * SS, H * SS))
    d = ImageDraw.Draw(img)

    cx = W * SS // 2

    logo = 104 * SS
    draw_logo(img, (cx - logo // 2, 40 * SS, logo))

    f_title = fit_font(d, "DeepHarness", latin_paths, 17 * SS, W * SS - 24 * SS)
    f_sub = fit_font(d, "零配置 AI Agent 工作台", cjk_paths, 8 * SS, W * SS - 20 * SS)
    f_item = fit_font(d, "三个 Agent 相互独立", cjk_paths, 7 * SS, W * SS - 24 * SS)

    def centered(text, font, y, color):
        d.text((cx - text_width(d, text, font) // 2, y * SS), text, font=font, fill=color)

    centered("DeepHarness", f_title, 176, TEXT)
    centered("零配置 AI Agent 工作台", f_sub, 200, TEXT_DIM)

    # 渐变分隔线
    line_w, line_y = 64 * SS, 220 * SS
    lx0 = cx - line_w // 2
    for i in range(line_w):
        d.line([(lx0 + i, line_y), (lx0 + i, line_y + SS)],
               fill=lerp(RING_FROM, RING_TO, i / max(line_w - 1, 1)))

    centered("三个 Agent 相互独立", f_item, 236, TEXT_FAINT)
    centered("开箱即用 · 本地运行", f_item, 252, TEXT_FAINT)

    d.line([(0, H * SS - 1), (W * SS, H * SS - 1)], fill=(48, 56, 74))

    img.resize((W, H), Image.LANCZOS).save(out_path, "BMP")
    return out_path


def main():
    ap = argparse.ArgumentParser(description="生成 NSIS 安装程序品牌素材")
    ap.add_argument("--out", default=os.path.join("src-tauri", "icons"),
                    help="输出目录（默认 src-tauri/icons）")
    args = ap.parse_args()

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out_dir = args.out if os.path.isabs(args.out) else os.path.join(root, args.out)
    os.makedirs(out_dir, exist_ok=True)

    cjk_paths = [
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Dengb.ttf",
    ]
    latin_paths = [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
    ]

    for name, fn in (
        ("installer-header.bmp", make_header),
        ("installer-sidebar.bmp", make_sidebar),
    ):
        p = fn(os.path.join(out_dir, name), cjk_paths, latin_paths)
        print("已生成 %s (%s)" % (p, "x".join(map(str, Image.open(p).size))))

    # 一并输出 PNG 预览，便于在编辑器里直接查看效果
    for name in ("installer-header", "installer-sidebar"):
        bmp = os.path.join(out_dir, name + ".bmp")
        png = os.path.join(out_dir, name + ".png")
        Image.open(bmp).save(png)
        print("预览图 %s" % png)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""生成桌面托盘 / 菜单栏图标（design/desktop/tray.html 的图标状态）。

四个状态：
  normal       品牌锁
  uploading    品牌锁 + 蓝色上传箭头
  paused       品牌锁 + 灰色暂停
  needs-unlock 品牌锁 + 橙色圆点

输出（apps/client/assets/tray/）：
  Windows：tray-<state>.ico，多尺寸 16/20/24/32/40/48
  macOS：  tray-<state>Template.png / @2x，黑色 + 透明，交给系统按菜单栏
           深浅色自动着色（`isIconTemplate`）。

只依赖 Pillow；不读取、不写入任何用户数据。运行：
  python apps/client/tool/gen_tray_icons.py
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

STATES = ("normal", "uploading", "paused", "needs-unlock")
ICO_SIZES = (16, 20, 24, 32, 40, 48)

BRAND = (0x00, 0x61, 0xFE, 0xFF)
WHITE = (0xFF, 0xFF, 0xFF, 0xFF)
PAUSE = (0x71, 0x71, 0x7A, 0xFF)
WARNING = (0xD9, 0x77, 0x06, 0xFF)
BLACK = (0, 0, 0, 255)

SCALE = 1024


def _base_tile(draw: ImageDraw.ImageDraw) -> None:
    """品牌蓝圆角方块 + 白色锁。"""
    draw.rounded_rectangle((56, 56, 760, 760), radius=170, fill=BRAND)
    # 锁梁（先画，锁体覆盖下半部分）。
    draw.rounded_rectangle(
        (310, 188, 550, 470), radius=120, outline=WHITE, width=66
    )
    # 锁体。
    draw.rounded_rectangle((256, 372, 604, 688), radius=62, fill=WHITE)
    # 锁孔 + 钥匙槽。
    draw.ellipse((398, 456, 462, 520), fill=BRAND)
    draw.rounded_rectangle((418, 498, 442, 586), radius=12, fill=BRAND)


def _overlay_badge(draw: ImageDraw.ImageDraw, color: tuple[int, int, int, int]) -> None:
    draw.ellipse((590, 590, 944, 944), fill=WHITE)
    draw.ellipse((630, 630, 904, 904), fill=color)


def _upload_arrow(draw: ImageDraw.ImageDraw, color: tuple[int, int, int, int]) -> None:
    # 上传箭头：竖杆 + 三角箭头。
    draw.rounded_rectangle((748, 682, 786, 860), radius=16, fill=color)
    draw.polygon(((690, 744), (844, 744), (767, 646)), fill=color)


def _pause_bars(draw: ImageDraw.ImageDraw, color: tuple[int, int, int, int]) -> None:
    draw.rounded_rectangle((700, 670, 744, 862), radius=20, fill=color)
    draw.rounded_rectangle((790, 670, 834, 862), radius=20, fill=color)


def _dot(draw: ImageDraw.ImageDraw, color: tuple[int, int, int, int]) -> None:
    draw.ellipse((700, 700, 834, 834), fill=color)


def windows_icon(state: str) -> Image.Image:
    image = Image.new("RGBA", (SCALE, SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    _base_tile(draw)
    if state == "uploading":
        _overlay_badge(draw, BRAND)
        _upload_arrow(draw, WHITE)
    elif state == "paused":
        _overlay_badge(draw, PAUSE)
        _pause_bars(draw, WHITE)
    elif state == "needs-unlock":
        draw.ellipse((590, 590, 944, 944), fill=WHITE)
        _dot(draw, WARNING)
    return image


def mac_template(state: str) -> Image.Image:
    """黑色 + 透明模板图；菜单栏按系统主题自动着色。"""
    image = Image.new("RGBA", (SCALE, SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    # 锁梁与锁体（实心剪影）。
    draw.rounded_rectangle(
        (300, 196, 540, 500), radius=118, outline=BLACK, width=72
    )
    draw.rounded_rectangle((244, 390, 596, 790), radius=70, fill=BLACK)
    # 锁孔用透明挖出，保证 22px 下仍能看出是锁。
    draw.ellipse((384, 486, 456, 558), fill=(0, 0, 0, 0))
    draw.rounded_rectangle((406, 534, 434, 640), radius=14, fill=(0, 0, 0, 0))
    if state == "uploading":
        # 箭头伸出锁体右下角。
        draw.rounded_rectangle((700, 646, 748, 900), radius=20, fill=BLACK)
        draw.polygon(((624, 700), (824, 700), (724, 566)), fill=BLACK)
    elif state == "paused":
        draw.rounded_rectangle((634, 620, 700, 900), radius=26, fill=BLACK)
        draw.rounded_rectangle((748, 620, 814, 900), radius=26, fill=BLACK)
    elif state == "needs-unlock":
        draw.ellipse((650, 650, 900, 900), fill=BLACK)
    return image


def main() -> None:
    output = Path(__file__).resolve().parent.parent / "assets" / "tray"
    output.mkdir(parents=True, exist_ok=True)
    written = 0
    for state in STATES:
        ico = windows_icon(state)
        ico.save(
            output / f"tray-{state}.ico",
            format="ICO",
            sizes=[(size, size) for size in ICO_SIZES],
        )
        written += 1

        template = mac_template(state)
        template.resize((22, 22), Image.LANCZOS).save(
            output / f"tray-{state}Template.png"
        )
        template.resize((44, 44), Image.LANCZOS).save(
            output / f"tray-{state}Template@2x.png"
        )
        written += 2
    print(f"generated {written} tray icon files in assets/tray/")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Deterministically rasterises the Oxidify icon.

The mark is drawn in code, not sampled from any image: six wave bars that
rust from blue through purple to orange, and the flakes falling off the
last one, on a transparent ground. Running this file regenerates
`packaging/windows/oxidify.ico` and `packaging/macos/icon-1024.png` exactly;
`oxidify.svg` is the same drawing in vector form, and `src/util.rs` holds
the same geometry for the window and tray icons.

    python packaging/icons/make_icon.py
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

GRADIENT = [(0x66, 0x66, 0xFF), (0xB0, 0x5A, 0x8A), (0xD2, 0x57, 0x2A)]
FLAKE = (*GRADIENT[2], 255)

CENTER = (88.75, 80.0)
SIDE = 148.0
BAR_RADIUS = 6.0
BARS = [
    (26.0, 68.0, 92.0),
    (46.0, 48.0, 112.0),
    (66.0, 30.0, 130.0),
    (86.0, 52.0, 108.0),
    (106.0, 38.0, 122.0),
    (126.0, 62.0, 98.0),
]
FLAKES = [(141.5, 55.5, 7.0, 20.0), (148.5, 76.5, 5.0, -15.0), (143.0, 103.0, 6.0, 35.0)]
DOT = (155.0, 90.0, 2.5)


def color_at(x: float) -> tuple[int, int, int]:
    t = min(max((x - 20.0) / 120.0, 0.0), 1.0) * 2.0
    start, end, local = (GRADIENT[0], GRADIENT[1], t) if t < 1.0 else (GRADIENT[1], GRADIENT[2], t - 1.0)
    return tuple(round(a + (b - a) * local) for a, b in zip(start, end))


def render(size: int) -> Image.Image:
    big = size * 4
    unit = big / SIDE
    left = CENTER[0] - SIDE / 2.0
    top = CENTER[1] - SIDE / 2.0

    def at(x: float, y: float) -> tuple[float, float]:
        return ((x - left) * unit, (y - top) * unit)

    bars = Image.new("L", (big, big), 0)
    draw = ImageDraw.Draw(bars)
    for x, y1, y2 in BARS:
        x0, y0 = at(x - BAR_RADIUS, y1 - BAR_RADIUS)
        x1, y1b = at(x + BAR_RADIUS, y2 + BAR_RADIUS)
        draw.rounded_rectangle([x0, y0, x1, y1b], radius=BAR_RADIUS * unit, fill=255)

    ramp = Image.new("RGBA", (big, 1))
    ramp.putdata([(*color_at(left + (column + 0.5) / unit), 255) for column in range(big)])
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    image.paste(ramp.resize((big, big), Image.NEAREST), (0, 0), bars)

    draw = ImageDraw.Draw(image)
    for cx, cy, side, degrees in FLAKES:
        angle = math.radians(degrees)
        half = side / 2.0
        corners = [
            at(cx + dx * math.cos(angle) - dy * math.sin(angle), cy + dx * math.sin(angle) + dy * math.cos(angle))
            for dx, dy in [(-half, -half), (half, -half), (half, half), (-half, half)]
        ]
        draw.polygon(corners, fill=FLAKE)
    dot_x, dot_y = at(DOT[0], DOT[1])
    r = DOT[2] * unit
    draw.ellipse([dot_x - r, dot_y - r, dot_x + r, dot_y + r], fill=FLAKE)

    return image.resize((size, size), Image.LANCZOS)


def main() -> None:
    render(256).save(
        ROOT / "packaging" / "windows" / "oxidify.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    render(1024).save(ROOT / "packaging" / "macos" / "icon-1024.png")
    print("wrote packaging/windows/oxidify.ico and packaging/macos/icon-1024.png")


if __name__ == "__main__":
    main()

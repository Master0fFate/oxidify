#!/usr/bin/env python3
"""Deterministically rasterises the Oxidify icon.

The mark is drawn in code, not sampled from any image: a light-blue field,
a deep-navy crystal cell, and one oxygen dot. Running this file regenerates
`packaging/windows/oxidify.ico` and `packaging/macos/icon-1024.png` exactly;
`oxidify.svg` is the same drawing in vector form.

    python packaging/icons/make_icon.py
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

FIELD = (145, 196, 255, 255)  # light blue   #91C4FF
INK = (13, 58, 115, 255)  # deep navy    #0D3A73

CENTER = (64.0, 64.0)
RADIUS = 42.0  # centre to a vertex of the crystal cell
STROKE = 9.0  # measured perpendicular to the cell's edges
DOT = 8.0  # the oxygen dot's radius


def hexagon(radius: float) -> list[tuple[float, float]]:
    """A pointy-top hexagon around the centre, vertex by vertex."""
    return [
        (
            CENTER[0] + radius * math.sin(math.radians(angle)),
            CENTER[1] - radius * math.cos(math.radians(angle)),
        )
        for angle in range(0, 360, 60)
    ]


def draw_mark(image: Image.Image) -> None:
    unit = image.width / 128.0
    draw = ImageDraw.Draw(image)

    def scaled(points):
        return [(x * unit, y * unit) for x, y in points]

    # The crystal cell: a navy ring with flat, parallel sides. The inner
    # hexagon is scaled about the centre so the ring is one even stroke.
    apothem = RADIUS * math.cos(math.radians(30))
    inner = RADIUS * (apothem - STROKE / 2) / apothem
    draw.polygon(scaled(hexagon(RADIUS)), fill=INK)
    draw.polygon(scaled(hexagon(inner)), fill=FIELD)

    # The oxygen dot at the centre.
    r = DOT * unit
    draw.ellipse(
        [64 * unit - r, 64 * unit - r, 64 * unit + r, 64 * unit + r], fill=INK
    )


def render(size: int) -> Image.Image:
    # Supersample for clean edges, then scale down.
    big = size * 4
    image = rounded_square(big)
    draw_mark(image)
    if size != big:
        image = image.resize((size, size), Image.LANCZOS)
    return image


def rounded_square(size: int) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=round(size * 28 / 128), fill=FIELD
    )
    return image


def main() -> None:
    # The .ico Windows shows in Explorer and the taskbar.
    render(256).save(
        ROOT / "packaging" / "windows" / "oxidify.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    # The 1024px PNG macOS iconsets are built from.
    render(1024).save(ROOT / "packaging" / "macos" / "icon-1024.png")
    print("wrote packaging/windows/oxidify.ico and packaging/macos/icon-1024.png")


if __name__ == "__main__":
    main()

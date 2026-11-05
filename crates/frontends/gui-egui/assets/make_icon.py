"""Writes noido.svg: a lowercase n in Computer Modern math italic (the n of
$n$ in TeX), white on a squircle of deep leaf green.

The squircle is Apple's shape: straight sides, and corners whose curve
eases in, as Figma's corner smoothing draws them (radius 22.5 % of the
side, 185 of Apple's 824 tile; smoothing 60 %). Each corner is a Bezier
into a circular arc and a Bezier out of it. The side is full-bleed;
build.rs adds macOS's margin and shadow for the .icns.

The SVG is the icon's source; build.rs renders what the app needs from it.
This script only records how it was made. The font is not in the repo:
matplotlib ships it (mpl-data/fonts/ttf/cmmi10.ttf).

    uv run --with fonttools python make_icon.py path/to/cmmi10.ttf > noido.svg
"""

import math
import sys

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont

SIDE = 1024
CORNER_RADIUS = 0.225  # of the side
CORNER_SMOOTHING = 0.6
PADDING = 0.22  # of the side, around the letter's longer extent
BACKGROUND = "#0f3302"
LETTER = "#ffffff"


def squircle() -> str:
    """The tile's outline as an SVG path: Figma's smoothed rounded rect,
    after figma-squircle's getPathParamsForCorner."""
    r = SIDE * CORNER_RADIUS
    xi = CORNER_SMOOTHING
    p = (1 + xi) * r  # how much of each side the corner takes
    arc = 90 * (1 - xi)  # degrees of the circular part
    arc_len = math.sin(math.radians(arc / 2)) * r * math.sqrt(2)
    alpha = (90 - arc) / 2
    beta = 45 * xi
    c = r * math.tan(math.radians(alpha / 2)) * math.cos(math.radians(beta))
    d = c * math.tan(math.radians(beta))
    b = (p - arc_len - c - d) / 3
    a = 2 * b
    n = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")  # noqa: E731

    def corner(sx: int, sy: int, along_x: bool) -> str:
        """One corner, relative, turning clockwise. Along x first means
        it starts on a horizontal side."""

        def pt(u: float, v: float) -> str:
            # u along the side it starts on, v across it.
            x, y = (u * sx, v * sy) if along_x else (v * sx, u * sy)
            return f"{n(x)} {n(y)}"

        return (
            f"c{pt(a, 0)} {pt(a + b, 0)} {pt(a + b + c, d)}"
            f"a{n(r)} {n(r)} 0 0 1 {pt(arc_len, arc_len)}"
            f"c{pt(d, c)} {pt(d, b + c)} {pt(d, a + b + c)}"
        )

    return (
        f"M{n(SIDE - p)} 0"
        + corner(1, 1, True)
        + f"L{n(SIDE)} {n(SIDE - p)}"
        + corner(-1, 1, False)
        + f"L{n(p)} {n(SIDE)}"
        + corner(-1, -1, True)
        + f"L0 {n(p)}"
        + corner(1, -1, False)
        + "Z"
    )


def main(font_path: str) -> None:
    font = TTFont(font_path)
    glyphs = font.getGlyphSet()
    glyph = glyphs[font.getBestCmap()[ord("n")]]

    bounds = BoundsPen(glyphs)
    glyph.draw(bounds)
    x0, y0, x1, y1 = bounds.bounds
    path = SVGPathPen(glyphs)
    glyph.draw(path)

    # Fit the letter's box, centred; font units have y up.
    scale = SIDE * (1 - 2 * PADDING) / max(x1 - x0, y1 - y0)
    tx = SIDE / 2 - scale * (x0 + x1) / 2
    ty = SIDE / 2 + scale * (y0 + y1) / 2
    print(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIDE} {SIDE}" '
        f'width="{SIDE}" height="{SIDE}">\n'
        f'<path fill="{BACKGROUND}" d="{squircle()}"/>\n'
        f'<path transform="translate({tx:.2f} {ty:.2f}) scale({scale:.5f} {-scale:.5f})" '
        f'fill="{LETTER}" d="{path.getCommands()}"/>\n'
        "</svg>"
    )


if __name__ == "__main__":
    main(sys.argv[1])

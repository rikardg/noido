"""Writes noido.svg: a lowercase n in Computer Modern math italic (the n of
$n$ in TeX), white on a rounded square of deep leaf green.

The SVG is the icon's source; build.rs renders what the app needs from it.
This script only records how it was made. The font is not in the repo:
matplotlib ships it (mpl-data/fonts/ttf/cmmi10.ttf).

    uv run --with fonttools python make_icon.py path/to/cmmi10.ttf > noido.svg
"""

import sys

from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont

SIDE = 1024
RADIUS = SIDE * 0.2
PADDING = 0.22  # of the side, around the letter's longer extent
BACKGROUND = "#0f3302"
LETTER = "#ffffff"


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
        f'<rect width="{SIDE}" height="{SIDE}" rx="{RADIUS:g}" fill="{BACKGROUND}"/>\n'
        f'<path transform="translate({tx:.2f} {ty:.2f}) scale({scale:.5f} {-scale:.5f})" '
        f'fill="{LETTER}" d="{path.getCommands()}"/>\n'
        "</svg>"
    )


if __name__ == "__main__":
    main(sys.argv[1])

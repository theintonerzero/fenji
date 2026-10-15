# Fenji's seal as an SVG: the ember-600 rounded tile with 焚 in white, the glyph's outline taken
# from Noto Serif CJK SC at weight 700 (Noto Serif SC Bold), as DESIGN.md's header seal draws it:
# a 26 px tile, 6 px radius, the glyph at 14 px, here scaled to 1024 px.
import sys
from fontTools.ttLib import TTCollection
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen

ttc, out = sys.argv[1], sys.argv[2]
font = TTCollection(ttc).fonts[2]                     # Noto Serif CJK SC
assert font["name"].getDebugName(1) == "Noto Serif CJK SC"
font = instantiateVariableFont(font, {"wght": 700})   # Bold
glyph = font.getBestCmap()[ord("焚")]
glyphs = font.getGlyphSet()
pen = SVGPathPen(glyphs)
glyphs[glyph].draw(pen)
bounds = BoundsPen(glyphs)
glyphs[glyph].draw(bounds)
x0, y0, x1, y1 = bounds.bounds
upm = font["head"].unitsPerEm

SIZE = 1024
radius = round(SIZE * 6 / 26)
scale = SIZE * 14 / 26 / upm                          # the em at 14/26 of the tile
# Centre the glyph's ink in the tile; font units are y-up, SVG's y-down.
tx = SIZE / 2 - (x0 + x1) / 2 * scale
ty = SIZE / 2 + (y0 + y1) / 2 * scale
svg = (
    f'<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" '
    f'viewBox="0 0 {SIZE} {SIZE}">\n'
    f'  <rect width="{SIZE}" height="{SIZE}" rx="{radius}" fill="#c8431f"/>\n'
    f'  <path fill="#ffffff" transform="translate({tx:.2f} {ty:.2f}) scale({scale:.6f} {-scale:.6f})" '
    f'd="{pen.getCommands()}"/>\n'
    "</svg>\n"
)
open(out, "w").write(svg)
print(f"{glyph}: bounds {bounds.bounds}, upm {upm}, scale {scale:.4f}, radius {radius}")

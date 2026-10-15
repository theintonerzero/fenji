# The launcher's icons

Every icon here is Fenji's seal, as [DESIGN.md](../../../../DESIGN.md) draws it in the header: an ember-600 (`#c8431f`) tile with 6 px corners on 26 px, and 焚 in white, Noto Serif SC 700 at 14 px. Here it is scaled to 1024 px: corners of 236 px, the glyph's em at 551 px, its ink centred in the tile.

- [`seal.svg`](seal.svg) is the source: the tile, and the glyph's outline as one path.
- [`seal.py`](seal.py) made it. It takes the outline of 焚 (U+711A) from Noto Serif CJK SC, instanced at weight 700 (Noto Serif SC Bold), with fonttools' `SVGPathPen`.
- The rest (`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, `icon.icns`, `icon.ico`) came from `cargo tauri icon`, run on a 1024 px PNG of the SVG. `tauri.conf.json`'s `bundle.icon` lists the ones the bundles use.

The font is Noto Serif CJK, under the SIL Open Font License 1.1; the seal uses one glyph's outline from it. Nothing here comes from the game.

## Making them again

The 2026-10-15 run used fonttools 4.67.0, Fedora's `google-noto-serif-cjk-vf-fonts` 2.003, ImageMagick with librsvg 2.62.3 (the renderer behind `rsvg-convert`), and tauri-cli 2.12.2. From this folder, in a scratch folder of your own:

```sh
uv venv /tmp/seal/.venv && uv pip install --python /tmp/seal/.venv/bin/python fonttools==4.67.0
/tmp/seal/.venv/bin/python -I seal.py /usr/share/fonts/google-noto-serif-cjk-vf-fonts/NotoSerifCJK-VF.ttc seal.svg
magick -background none RSVG:seal.svg -resize 1024x1024 PNG32:/tmp/seal/seal-1024.png
cargo tauri icon /tmp/seal/seal-1024.png -o .
rm -rf android ios Square*Logo.png StoreLogo.png 64x64.png   # the launcher is for desktops only
```

`rsvg-convert -w 1024 -h 1024 seal.svg -o /tmp/seal/seal-1024.png` gives the same PNG where librsvg's own tool is installed.

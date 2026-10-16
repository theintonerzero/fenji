# The launcher's fonts

Every font here is under the SIL Open Font License 1.1, whose text sits beside it. They ship inside the launcher, since its content security policy (`default-src 'self'`) loads nothing from a font CDN (DESIGN.md, Typography). Each file is variable on the weight axis.

| File | Family | Source | Version | SHA-256 |
|---|---|---|---|---|
| `Geist.woff2` | Geist | `geist-font/Geist/webfonts/Geist[wght].woff2` in [vercel/geist-font v1.7.2](https://github.com/vercel/geist-font/releases/tag/v1.7.2)'s `geist-font-v1.7.2.zip` (SHA-256 `7fc800d2ac6b92844895196e5041aca55d814c15db70c44f79b3b83ab82b04e2`), unchanged | 1.800 | `a369fcf5628ea2aa4e1b9e2ec6a5b3624e365bda588e1f0f2f12b564f728fbb8` |
| `GeistMono.woff2` | Geist Mono | `geist-font/GeistMono/webfonts/GeistMono[wght].woff2` in the same zip, unchanged | 1.700 | `fba8f577f38a2bbcbe818efa6348dd58f36303a10b8737c42fefad275be563ab` |
| `Cinzel.woff2` | Cinzel | [`ofl/cinzel/Cinzel[wght].ttf`](https://github.com/google/fonts/blob/b7c8331cf620346575487ef8db0e2c729f014675/ofl/cinzel/Cinzel%5Bwght%5D.ttf) in google/fonts at commit `b7c8331cf620346575487ef8db0e2c729f014675` (SHA-256 `f4d83d34d1f6c741193e4acf4b3dff9531e5a67b6aa65228d00a7db72a4e0f34`), subset to Latin-1 and the common punctuation | 2.000 | `7bd713c9098e85a741dd9620f854b7f8db7294598c63378441d103639bcb6bb9` |
| `NotoSerifSC-fenji.woff2` | Noto Serif SC | [`ofl/notoserifsc/NotoSerifSC[wght].ttf`](https://github.com/google/fonts/blob/b7c8331cf620346575487ef8db0e2c729f014675/ofl/notoserifsc/NotoSerifSC%5Bwght%5D.ttf) at the same commit (SHA-256 `050080d9255a86808f2945bffac582b31ef32bc36411ce29563b4961670c66f9`), subset to the two glyphs 焚寂 | 2.003 | `8ae63c62769f8b814f1d07f243aff03e946cd8a157c9f8786a9e47acbb47c2ac` |

The licences: `OFL-Geist.txt` (from the same zip), `OFL-Cinzel.txt` and `OFL-NotoSerifSC.txt` (from google/fonts at the same commit).

## Making them again

With fonttools 4.67.0 and brotli 1.2.0 (`pip install fonttools==4.67.0 brotli==1.2.0`, never committed), from the downloaded sources:

```sh
pyftsubset 'Cinzel[wght].ttf' --unicodes='U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-203A,U+20AC,U+2122' \
  --layout-features='*' --flavor=woff2 --output-file=Cinzel.woff2
pyftsubset 'NotoSerifSC[wght].ttf' --text='焚寂' --layout-features='*' --flavor=woff2 \
  --output-file=NotoSerifSC-fenji.woff2
```

Geist and Geist Mono need no conversion: their release ships variable WOFF2 files.

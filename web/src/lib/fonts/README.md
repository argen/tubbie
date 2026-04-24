# Bundled Fonts — Licence Attribution

All fonts are self-hosted under the **SIL Open Font Licence (OFL) 1.1**.
They must never be served from a CDN; they are bundled in this directory
and loaded via `@font-face` with `font-display: block` in `app.css`.

## Geist Pixel Square / Circle

- **Files**: `GeistPixel-Square.woff2`, `GeistPixel-Circle.woff2`
- **Licence text**: `GeistPixel-OFL.txt`
- **Authors**: Vercel
- **Source**: https://github.com/vercel/geist-font (v1.8.0)
- **Licence**: SIL Open Font Licence 1.1 — https://openfontlicense.org/open-font-license-official-text/
- **Use in tubbie**: primary dot-matrix display font. `Square` is the default
  (exposed as `--font-board`); `Circle` is bundled for future per-theme
  variant switching and is not currently referenced by any stylesheet.

## VT323

- **File**: `VT323.woff2`
- **Authors**: Peter Hull
- **Source**: https://fonts.google.com/specimen/VT323
- **Licence**: SIL Open Font Licence 1.1 — https://openfontlicense.org/open-font-license-official-text/
- **Use in tubbie**: fallback behind Geist Pixel Square in `--font-board`.
  Retained for one release as a safety net for any codepoint Geist Pixel
  does not cover.

## SIL OFL 1.1 Summary

Permission is granted to use, study, copy, merge, embed, modify, redistribute,
and sell the fonts, subject to the following conditions:

1. Neither the font nor any component may be sold by itself.
2. The fonts, modified or unmodified, must be distributed with the above
   copyright and this licence, and the Reserved Font Name(s) must not be used.
3. The fonts and derivatives must not be released under any other type of
   licence.

The full licence text is at https://openfontlicense.org/open-font-license-official-text/

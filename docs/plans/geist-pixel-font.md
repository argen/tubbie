# Geist Pixel Font Refactor — Plan

Working branch (not yet opened): `ui/geist-pixel-font` → PR for visual review before merging.

## Font

**Geist Pixel** (Vercel, v1.8.0 — March 2026). OFL 1.1, safe to bundle. Five pixel-grid variants: Square / Grid / Circle / Triangle / Line. Vector font crafted on a pixel grid (not raster bitmap). Distributed via `geist` npm package, Fontsource, and direct WOFF2 download from GitHub releases. ~20–30 KB WOFF2 per variant.

- Repo: https://github.com/vercel/geist-font
- Releases: https://github.com/vercel/geist-font/releases/latest
- Blog: https://vercel.com/blog/introducing-geist-pixel

## Why it fits Tubbie

Current font is **VT323** (classic CRT terminal). Geist Pixel **Square** reads closer to real LED-matrix displays; **Grid** / **Circle** look more like actual Underground Dot Matrix Indicators.

## Implementation steps

1. **Licensing / bundling**
   - Pull WOFF2 files directly from GitHub release tarball — avoid npm dep for a font-only change.
   - Drop into `web/static/fonts/geist-pixel/` alongside the OFL 1.1 `LICENSE`.

2. **CSS wiring** (`web/src/app.css` — single source of truth)
   - Add `@font-face` for Geist Pixel Square (primary) and one contrast variant (Circle) for future per-theme switching.
   - Use `font-display: block` to avoid FOIT/FOUT flash.
   - Introduce `--font-board: 'Geist Pixel Square', 'VT323', monospace;`. Keep VT323 as fallback for one release.
   - Replace hardcoded `'VT323', monospace` across all `.svelte` components (Board, ArrivalRow, PlatformColumn, LineStatusTicker, loading/error states, settings, theme picker) with `var(--font-board)`.
   - Add `-webkit-font-smoothing: antialiased;` on `body` to avoid WebKit subpixel blur on macOS (WKWebView).
   - Switch board font sizes from `rem` to integer `px` so glyphs align to the pixel grid.

3. **Theme coupling** *(optional this PR, follow-up OK)*
   - Extend theme definitions so each theme picks a variant: classic-amber → Square, high-contrast → Grid, modern-white → Circle. Font becomes part of the theme identity.

4. **CSP**
   - No change. `font-src 'self'` already permits bundled fonts.

5. **Build hygiene**
   - No bundler config change (SvelteKit static assets work out of the box).
   - Check bundled `.app` size delta — expect +~30–60 KB for two variants. Call out in PR.

6. **QA checklist** (to include in PR body)
   - All four themes render with new font at intended sizes; no overflow.
   - Character-reveal animation readable (single-glyph visibility per tick).
   - Marquee ticker kerning correct.
   - "Due" pulse / blink doesn't smear.
   - Reduced-motion path unchanged.
   - Screenshot each theme before/after.

7. **Rollback**
   - Revert the branch. VT323 fallback in `--font-board` keeps the app usable even on partial revert.

## Open questions for Bruno

- Preferred default variant: Square (most readable) or Grid (most Underground-ish)?
- Scope: typography-only PR, or include per-theme variant coupling?

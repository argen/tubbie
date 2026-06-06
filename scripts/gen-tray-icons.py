#!/usr/bin/env python3
"""Generate the menubar tray icons for tubbie.

The mark is an original **dot-matrix** glyph (a small LED dot grid, echoing the
departure-board UI) — deliberately NOT the TfL roundel (a registered trademark)
and not a circle-with-a-bar (which reads as a "no entry" sign at menubar size).

Output: macOS *template* images — opaque black on a transparent background.
macOS uses the alpha channel as a mask and tints the icon to the menubar
foreground (white on a dark bar, black on a light one), so colour is irrelevant;
only the silhouette matters.

  normal : a 5x3 grid of dots (the LED matrix).
  alert  : a dot-matrix exclamation (a stacked bar + dot) — a distinct
           silhouette, not merely a recoloured normal icon, so it reads even
           when the menubar tint is identical.

Rendered at 8x then downsampled (LANCZOS) for crisp anti-aliased edges at the
22px / 44px (@2x) menubar sizes.

Run from the repo root:  python3 scripts/gen-tray-icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

BLACK = (0, 0, 0, 255)
SS = 8  # supersample factor
OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def _canvas(size: int) -> tuple[Image.Image, ImageDraw.ImageDraw, int]:
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img), s


def draw_normal(size: int) -> Image.Image:
    """A letter "T" rendered in a 5x5 LED dot grid — the tubbie monogram in
    dot-matrix form (top row lit + a centre stem)."""
    img, d, s = _canvas(size)
    cols, rows = 5, 5
    pad = round(s * 0.15)
    # Fatter dots (nearly touching, grid spacing is 0.175*s) so the T reads
    # bold at menubar size rather than thin and spindly.
    dot_r = round(s * 0.084)
    span_x = s - 2 * pad
    span_y = s - 2 * pad

    def dot(col: int, row: int) -> None:
        x = pad + span_x * col / (cols - 1)
        y = pad + span_y * row / (rows - 1)
        d.ellipse([x - dot_r, y - dot_r, x + dot_r, y + dot_r], fill=BLACK)

    lit = [(c, 0) for c in range(cols)]  # top bar
    lit += [(cols // 2, r) for r in range(1, rows)]  # centre stem
    for col, row in lit:
        dot(col, row)
    return img.resize((size, size), Image.LANCZOS)


def draw_alert(size: int) -> Image.Image:
    """Dot-matrix exclamation — a stacked bar + dot, centred."""
    img, d, s = _canvas(size)
    cx = s // 2
    pad = round(s * 0.17)
    dot_r = round(s * 0.085)
    bar_w = dot_r
    bar_top = pad
    bar_bot = s - pad - round(s * 0.24)
    d.rounded_rectangle(
        [cx - bar_w, bar_top, cx + bar_w, bar_bot], radius=bar_w, fill=BLACK
    )
    by = s - pad - dot_r
    d.ellipse([cx - dot_r, by - dot_r, cx + dot_r, by + dot_r], fill=BLACK)
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, fn in [("tray-icon", draw_normal), ("tray-icon-alert", draw_alert)]:
        fn(22).save(OUT / f"{name}.png")
        fn(44).save(OUT / f"{name}@2x.png")
        print(f"wrote {name}.png (22) + {name}@2x.png (44)")


if __name__ == "__main__":
    main()

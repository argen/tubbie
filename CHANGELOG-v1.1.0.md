# Tubbie 1.1

A polish release that makes Settings feel native to the app, fixes the links,
and turns a confusing location failure into a one-tap fix.

## Settings now lives inside the app

Settings used to open in a separate window. It now slides in over the board —
the same in-app feel as the search bar and the status view — and works the same
way whether you're in window mode or the menu-bar popover. Press **Escape** or
**← Back** to return to the board. Switching display mode from Settings now
re-shapes the one window instead of leaving a second window stranded behind it.

## Fixes

- **Theme picker.** The four theme cards rendered as plain grey system buttons;
  they now show the actual dot-matrix board colours, with the selected one
  glowing in its own theme colour.
- **Links work.** "Powered by TfL Open Data", the API-key portal link, and the
  About section's "Source & releases" / "TfL Open Data" links now open in your
  browser (and are styled to match the board) instead of doing nothing.
- **Find nearest station.** When Location Services is off for Tubbie, the search
  bar now says **"Location off — open Settings"** and takes you straight to the
  right System Settings pane in one tap, instead of spinning for eight seconds
  and reporting a misleading "no signal". When it's on, it finds your nearest
  station as before.

## Accessibility

- The in-app Settings panel is a proper modal: focus moves into it on open and
  returns to where you were on close, and the board behind it is inert while
  it's up.

---

**Install:** download the `.dmg`, drag Tubbie to Applications, and launch. The
build is signed with a Developer ID and notarized by Apple. Existing installs
update in-app.

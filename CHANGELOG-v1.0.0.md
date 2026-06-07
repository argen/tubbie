# Tubbie 1.0

Tubbie's first stable release — and its biggest update yet. Tubbie now works
the moment you open it, covers the whole TfL network, and turns the menu bar
into a genuine at-a-glance surface.

## Highlights

- **Zero-config setup.** A fresh install shows live arrivals straight away —
  no TfL API key to register or paste. Tubbie fetches and applies a shared key
  silently, and if that's ever unavailable it quietly falls back to anonymous
  access; the board is never blocked waiting on a key. You can still set your
  own key in Settings, and it always takes priority.
- **Network-wide service status.** The Status panel and full Status view now
  cover *every* TfL line, not just the ones at your current station. Disrupted
  lines are listed worst-first with the affected stretch of route
  (e.g. "Watford ↔ Harrow") or "Entire line", a tap to expand the full reason,
  and a calm "Good service across the network" when everything's running.
- **Search from the board.** Change station without opening Settings — a search
  bar drops down right below the header.
- **Redesigned menu bar.** A new original dot-matrix tray icon that stays crisp
  and legible on both light and dark menu bars, and switches to an alert glyph
  the moment a line you're watching is disrupted. The old next-arrival ETA text
  is gone — it read as clutter; the icon itself opens the popover.
- **Friendlier first run.** A single, skippable prompt to pick your station on
  first launch, plus a new About section in Settings.

## Improvements

- Two-row menu-bar header so long station names (Tottenham Court Road,
  Highbury & Islington) no longer truncate.
- The clock keeps seconds.
- Line colours pinned to Tubbie's canonical palette, matching the iOS app.

## Fixes

- The Elizabeth line header no longer reads "Elizabeth Line Line".
- The DLR and the Overground network no longer get an incorrect "Line" suffix
  (they're a railway and a network, not lines).
- The station-search bar opens flush below the header instead of overlapping
  the clock and buttons.

## Under the hood

- Affected-route segments were added to Tubbie's shared core, so the same data
  powers both the desktop and iOS apps.
- Debug builds now read the API key from an environment variable instead of the
  macOS Keychain, ending the repeated password prompts during development;
  release builds are unchanged and still use the Keychain.

---

**Install:** download the `.dmg`, drag Tubbie to Applications, and launch. The
build is signed with a Developer ID and notarized by Apple. Updates install
in-app from here on.

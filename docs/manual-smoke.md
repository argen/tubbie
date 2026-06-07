# Manual smoke checklists

Visual smoke testing is required for the changes called out below;
production timing differs from any single dev interaction. **Run with
`cargo tauri dev`.**

The pipeline tests in `cargo test --workspace` cover the Rust side, and
`cd web && npm test` covers the frontend — but neither catches "the
window resized to a degenerate size" or "the tray icon didn't redraw".

## Stream / config changes

For any change to `save_config_inner`, `BoardService::stream`,
`AppState`, the `watch::Sender`/`Receiver` wiring, or `spawn_stream_task`:

- **Switch station** to a multi-line station (King's Cross). Board
  updates within ~1 s. Tail the dev log for `stream tick recovered` and
  for repeated 429s.
- **Rapid-toggle 6+ chips** in Settings. Board MUST NOT flicker; the log
  MUST NOT show stream respawn.
- **Force a station swap then return: A → B → A.** Board emits each in
  turn (B briefly visible, then A).
- **Drop `poll_seconds` 30 → 60** via the slider. Next tick fires ~60 s
  later. No stream restart in logs.

## Display-mode changes

For changes to `apply_display_mode_effects`, `save_display_mode`, or the
`display_mode` lock:

- **Window → menubar:** dock icon disappears, window hides, tray icon
  appears in the menu bar within ~1 s. Left-click the tray → popover
  shows under the icon at 380×560.
- **Menubar → window:** tray icon disappears, dock icon reappears,
  window shows at 980×720 centered with the LED title bar (no native
  chrome).
- **Toggle 5× rapidly.** No crashes, no duplicate trays, final state
  matches the last toggle.
- **Mid-tick mode swap.** `board://updated` MUST keep flowing — the
  mode swap touches no stream state.
- **Mode swap, then change station.** `save_config` still works (no
  lock starvation, no leaked Arc cycle).

## Adaptive resize / line-grouped layout

For changes to `apply_board_size`, `Board.svelte::pickBoardSize`, or the
`linesGrouped` derivation. Cycle through this set of stations to cover
every line-count tier and the multi-line corner cases:

- **1-line** (Belsize Park, Stockwell): menubar 380×520, window 700×560.
  One LINE header, two direction columns under it.
- **2-line** (Oxford Circus, Green Park): menubar 380×620, window
  980×680. Two LINE headers stacked, two directions each.
- **3-line** (Tottenham Court Road, Bank): menubar 380×720, window
  1200×760. Three LINE headers stacked.
- **4+ line** (Baker Street — Metropolitan / Bakerloo / Circle / H&C /
  Jubilee — or King's Cross): menubar 380×800, window 1200×880.
  **Critical: every line stripe on every row must match its line
  header** — no Bakerloo orange under the Metropolitan group, no
  Jubilee silver under Bakerloo. Mixed stripes mean invariant #11
  (frontend per-line re-bucketing) regressed.
- **Back to a 1-line station.** Window/popover shrinks in a single
  resize step (no flicker, no intermediate sizes).
- **Watch the dev log.** One `apply_board_size` invocation per tier
  transition; sitting on the same station for 60 s (two poll ticks)
  MUST NOT issue extra resize requests. The renderer-side dedupe
  (`lastSizeKey`) is what protects the main-thread Cocoa dispatch.

## Menubar tray icon

For changes to the tray icon assets (`scripts/gen-tray-icons.py`,
`icons/tray-icon*.png`) or `apply_tray_disruption`. `TrayIcon::set_icon`
reaches `NSStatusItem` — a main-thread-only Cocoa call (invariants #8/#9), so
the swap CANNOT be covered by `cargo test`; a wrong-thread dispatch crashes
with `EXC_BREAKPOINT`, not a test failure.

There is **no ETA text in the menu bar** — the tray is icon-only and the icon
is the click target that opens/hides the popover. (The earlier menubar
live-arrival title was removed: it read as confusing clutter.)

- **Tray icon legibility.** The icon is an original monochrome **dot-matrix**
  glyph (`icons/tray-icon.png` @1x + `@2x`) — a 5×3 LED dot grid, echoing the
  departure-board UI; deliberately NOT the TfL roundel (a trademark). Confirm
  it reads as a small dot grid (not a muddy blob), tints correctly on a
  **light** AND **dark** menu bar, and stays crisp beside the notch and under
  Reduce-Transparency. On a Retina Mac the @2x is what shows.
- **Click to open.** Left-click the tray icon → the popover shows; click again
  → it hides. No ETA or text appears beside the icon.
- **Disruption icon swap.** When a line in your selection is disrupted, the
  tray icon swaps to the dot-matrix **alert** glyph (an exclamation,
  `icons/tray-icon-alert.png`); back to the dot grid when all clear. Both are
  templates — confirm both tint on light/dark and that the swap doesn't crash
  (`set_icon` is a main-thread Cocoa call, invariants #8/#9). The swap is
  driven by the frontend statuses (scoped to your line filter), so it still
  updates with the popover closed; it should NOT re-dispatch every poll when
  the disruption state is unchanged.
- **Switch menubar → window.** The tray icon disappears with no orphaned
  status item and no crash; switching back, it reappears within a poll.
- **Regenerate the assets.** After editing `scripts/gen-tray-icons.py`, run
  `python3 scripts/gen-tray-icons.py` and re-check legibility at both sizes
  and both menu-bar appearances.

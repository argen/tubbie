# Manual smoke checklists

Visual smoke testing is required for the changes called out below;
production timing differs from any single dev interaction. **Run with
`./dev`** (the one-command launcher — no global Tauri CLI needed).

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

## Station search from the board

For changes to `StationSearch.svelte`, the board-header search overlay, or the
debounced `search_stations` IPC call:

- **Open search.** Click the search / change-station button in the board header.
  A `StationSearch` input drops in below the header. The button's label changes
  to "Close station search".
- **Search and select.** Type a station name. Results appear after the debounce
  window. Click a result — the board immediately begins loading arrivals for that
  station and the search overlay closes.
- **Keyboard dismiss.** Press Escape while the search is open — the overlay
  closes without changing the station.
- **Settings search unchanged.** The same `StationSearch` component in the
  Settings "Station" section must still work independently.

## First-run prompt

For changes to `FirstRunPrompt.svelte` or the `onboarded` persistence logic:

- **First launch (clean state).** With no `onboarded` flag in the config store,
  the "Welcome to Tubbie" banner appears above the board. The board underneath
  is already showing live arrivals (prompt is not a gate).
- **Select station from prompt.** Pick a station in the prompt's search. The
  station is saved, the prompt disappears, and the board updates.
- **Dismiss with ×.** Click the × button. The prompt disappears and the
  `onboarded` flag is set so it won't reappear.
- **Dismiss with Escape.** Press Escape while the prompt is visible. Same
  result as ×.
- **Second launch.** Re-launch the app. The prompt must NOT reappear.

## Service status (StatusPanel + StatusView)

For changes to `get_all_line_statuses`, `StatusPanel.svelte`, `StatusView.svelte`,
`utils/status.ts`, or the `line_status_cache` fan-out in `tfl-client`:

- **Marquee ticker.** With at least one disrupted line active, the ticker at
  the bottom of the board scrolls smoothly left and loops. Each disrupted line
  shows its colour stripe, name, and worst-bucket label. "Good service on all
  other lines" appears at the end of the loop. Under `prefers-reduced-motion`,
  confirm the ticker is replaced by a static list.
- **Toggle Status view.** Click the "Status" button in the board header. The
  arrivals area is replaced by the full status view; the header button appears
  pressed. Clicking again restores arrivals.
- **Full status view content.** Each disrupted line should show a left colour
  stripe, bold line name, severity sub-headline (e.g. "Minor Delays"), and
  affected route segments ("Watford ↔ Harrow") or "Entire line" when no segments
  are present. The disclosure `›` chevron expands the full disruption prose.
  A "Good service on all other lines" bar appears when at least one line is
  undisrupted. "Service status unavailable." appears if no data was returned.
- **"All lines good" state.** With no network disruptions, the ticker shows
  only the green-dot "Good service across the network" line (no marquee).
  The full status view shows "All lines good" in the count label and no list.
- **Refresh button.** In the full status view, click "Refresh". The "Updated X
  min ago" footer label resets to "Updated just now". No crash; no reload of
  the arrivals stream.

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

# CLAUDE.md — agent guidance for tubbie

> Every rule here exists because something broke. Add to it when something breaks again.

## What tubbie is

Tauri v2 desktop app showing live TfL arrivals. Rust in `src-tauri/` and
`crates/tfl-*`, SvelteKit + Svelte 5 runes in `web/`. See
[`docs/architecture.md`](docs/architecture.md) for the crate graph and module
boundaries.

## The pipeline you'll keep breaking

```
Settings UI                  Tauri command      AppState        Stream task          board://updated
toggleLine() / chip / etc.   save_config        cfg_tx.send  →  cfg_rx.changed()  →  app.emit(...)
        │                       │                                   │
   persistDebounced (400 ms)    save to store                  refresh + last_ok logic
```

If you change `save_config_inner`, `BoardService::stream`, `AppState`, the
`watch::Sender`/`Receiver` wiring, or the `spawn_stream_task` respawn path,
run the integration tests below — not just the unit tests.

## Invariants

Don't violate without a test that proves the new contract. Numbers are
referenced from tests and PRs — don't renumber.

### Stream / config pipeline

1. **`save_config_inner` MUST publish to a live `watch::Receiver`.**
   `state.cfg_tx.send(cfg)` is `let _ = ...` so it fails silently when no
   receiver exists. `fixture_state()` drops `_cfg_rx`; use
   `fixture_state_with_stream(...)` for any test that asserts effects of
   `save_config` on the stream. The
   `fixture_state_with_stream_keeps_receiver_alive` test guards the helper
   itself.

2. **A `station_id` change MUST trigger an immediate refresh.** Up to 30 s
   of stale data after a deliberate click is unacceptable. Filter / theme /
   `poll_seconds` changes ride the next tick instead.
   Test: `stream_refreshes_immediately_on_station_id_change`.

3. **Filter / theme / `directions` changes MUST NOT trigger a fresh
   fetch.** Chip-toggle bursts must coalesce. Note: `line_ids` moved to
   frontend-only display filtering (#22) so it's no longer in
   `apply_filters`.
   Test: `save_config_filter_change_does_not_force_immediate_refresh`.

4. **The stream is infinite. The consumer MUST NOT `break` on `Err`.** On
   fetch failure with no `last_ok` it emits the error and keeps polling.
   Breaking at `spawn_stream_task` makes the watcher respawn the task every
   2 s, hammering TfL through any 429 cooldown. Log once per error streak
   and let `poll_seconds` throttle retries.

5. **`AppState` and the stream task MUST share one `Arc<TflClient>`.** That
   shares caches (`stop_points_cache`, `hub_children_cache`,
   `line_status_cache`), the connection pool, and the 429 cooldown
   process-wide. A fresh client on every spawn means a fresh 16 MB
   `/StopPoint/Mode/tube` warm.

6. **Initial config MUST load synchronously before the stream spawns.**
   `tauri::async_runtime::block_on(config_store.load_config())` avoids the
   race where the first tick refreshes the default station before an async
   loader publishes the saved one.

7. **`generated_at` is the latest-wins key in the frontend.** `applyBoard`
   in `web/src/lib/stores/board.ts` ignores any board whose `generated_at`
   is `<=` current. Re-emitting `last_ok` without bumping `generated_at`
   silently freezes the UI.

### Cocoa main-thread dispatch (macOS)

8. **Display-mode side-effects MUST run on the macOS main thread.**
   `apply_display_mode_effects_sync` calls `set_activation_policy`,
   `remove_tray_by_id` (whose returned `TrayIcon::Drop` calls
   `NSStatusBar::removeStatusItem`), `set_decorations`, `set_size`. Each
   asserts a Cocoa main-thread barrier
   (`BSServiceMainRunLoopQueue::assertBarrierOnQueue`) and crashes with
   `EXC_BREAKPOINT` if called from a Tokio worker. Tauri commands run on
   Tokio. Use the public async `apply_display_mode_effects` wrapper
   (`run_on_main_thread` + oneshot) from any non-`setup` caller. Setup
   itself is on the main thread and uses the sync version directly — the
   async one would deadlock because `run_on_main_thread`'s user event can
   only drain after setup returns.

9. **`apply_board_size` MUST hop to the main thread too.**
   `WebviewWindow::set_size` reaches `NSWindow::setFrame:display:` — same
   Cocoa assertion as #8. Use the public async `apply_board_size_effects`
   wrapper; the Tauri command calls it after `validate_board_size`.
   Validation runs *before* dispatch so a buggy renderer (NaN, infinity,
   out-of-range) never reaches Cocoa. The renderer dedupes per tier
   (`lastSizeKey` in `Board.svelte`); don't re-issue the resize from any
   other component — the Board owns it.

### Refresh / filter integrity

10. **Drop arrivals whose `line_id` is not served by the queried station.**
    TfL occasionally surfaces predictions under stop-points that don't
    physically serve that line (most likely path: hub-merge in
    `TflClient::get_arrivals`). The defensive filter
    `drop_arrivals_for_lines_not_serving` runs after `apply_filters`, using
    `TflClient::allowed_line_ids_for(station_id)` (which projects the
    hub-aware `Station.lines` field already populated by
    `stop_points_cached`). **Fail-open** when the cache is cold
    (`read_fresh_cache` returns `None`): allowed set is empty, filter is
    skipped — better to let one phantom through than drop legitimate
    arrivals on first refresh. The cache-warm task in `lib.rs::run`
    populates after the first emit. Disallowed arrivals log one warning
    per `(station, line)` per refresh on stderr with `[tfl-board]` —
    don't silence it, it's our only signal of upstream data drift.

11. **Line-grouped UI re-buckets per-line, not per-platform.** The Rust
    backend groups by `Direction` — `Board.platforms[]` has at most seven
    entries (Northbound … Unknown) and a single direction bucket mixes
    lines (King's Cross "Westbound" mixes hammersmith-city + metropolitan;
    Baker Street southbound mixes Bakerloo + Jubilee — see
    `refresh_groups_by_direction` in `board_service_tests.rs`). The
    frontend's `linesGrouped` derivation in `Board.svelte` walks every
    arrival, buckets by `line_id`, then by `direction` inside each line.
    Grouping by `Platform.arrivals[0].line_id` (the previous naive
    approach) silently mis-colours every minority-line train. The synthetic
    platform handed to `PlatformColumn` carries `name = direction.label`
    and the line+direction-filtered arrivals; the dedupe key
    `${line_id}|${platform_name}|${expected_arrival}` stays unique because
    backend-merged platforms differ on `platform_name`.

24. **Drop arrivals whose destination is the queried station itself.** At
    a terminus (Edgware, Mill Hill East, Stanmore, …) every inbound
    prediction has `destination_name == station_name` because the train
    physically terminates here. Showing them as "Northbound: Edgware" *at*
    Edgware is a tautology. Filter
    `drop_arrivals_terminating_at_queried_station` runs after
    `drop_off_axis_predictions`, comparing strings case-insensitively
    after trim. Fully data-driven (no per-station list) so any future
    terminus (W&C re-extension, branch closure short-working) is handled
    without code changes. Fail-open when either field is empty: TfL gave
    us no signal, hiding real trains is worse than showing a tautology.

### Multi-mode (tube / dlr / overground / elizabeth-line)

12. **Caches fetch per-mode and merge.** TfL's `/StopPoint/Mode/{mode}` and
    `/Line/Mode/{mode}/Status` accept a single mode each; comma-separated
    multi-mode is forbidden by the `[a-zA-Z0-9_-]{1,64}` validator in
    `crates/tfl-client/src/fixture.rs`, and widening it is the
    path-traversal regression `architecture.md` warns about. Stop-points
    cache is one merged `Vec<Station>` keyed by `Station.id` with line-list
    union on collision; line-status is one merged `Vec<TflLine>`. Both
    caches fan out across `tfl_client::SUPPORTED_MODES` in parallel via
    `futures::future::join_all`. Subset clients (iOS) use
    `TflClient::with_modes(http, &[&str])`. Adding a mode requires:
    (a) extend `SUPPORTED_MODES`, (b) record matching
    `fixtures/{stop-points,line-status}/{mode}.json`, (c) extend
    `is_supported_line_id` in `tfl-domain`, (d) extend the prefix whitelist
    in #13 if the NaPTAN scheme differs.

13. **Station search uses a NaPTAN canonical-prefix whitelist, not a
    `modes` list.** Allowed: `940GZZLU` (tube), `940GZZDL` (DLR), and
    `910G` filtered to stops whose `modes` include `overground` or
    `elizabeth-line` — the 910G NaPTAN range overlaps National Rail-only
    operators (Gatwick Express, Thameslink, Southern, …) which we don't
    surface. Excluded: `9400ZZLU*`, `4900*`, `2100*`, `HUB*` — platform
    children, NaPTAN bus-stop-at-station records, and aggregators with no
    stable arrivals id. New mode → extend whitelist + add a
    `search_stations_includes_<mode>_only_station` regression test.

14. **Legacy `"london-overground"` config ids migrate on load.** TfL
    stopped emitting predictions under the legacy id when the six named
    lines launched November 2024. `load_config_inner` calls
    `migrate_legacy_line_ids` (in `commands.rs`), which expands the legacy
    entry into `[liberty, lioness, mildmay, suffragette, weaver, windrush]`
    at the same position, deduping against existing entries. Idempotent.
    Keep `"london-overground"` accepted by `is_supported_line_id` so
    historical fixtures parse. Without this, upgrading users see a
    silently-empty Overground board. The same migration applies to
    `Favorite.lines` on favorites load.

23. **Elizabeth and the six named Overground lines surface as compass
    directions, not Inbound/Outbound.** TfL labels Elizabeth and OG
    platforms as bare `"Platform 3"` and `direction` only carries
    `inbound` / `outbound`. Without help, an Elizabeth train at Liverpool
    Street is bucketed as "Inbound" instead of "Eastbound". Fix:
    `infer_compass_from_towards` in `crates/tfl-domain/src/direction.rs`
    maps `towards` (the destination terminus) onto a compass direction.
    `infer_direction` order is: (1) platform_name prefix, (2) per-line
    `towards` mapping, (3) raw `direction`, (4) `Unknown`. DLR is
    intentionally out — its multi-branch topology (Bank/Tower Gateway W,
    Stratford N, Lewisham S, Beckton/Woolwich Arsenal E) doesn't fit a
    single per-terminus mapping. Adding a line: record termini in the
    docstring + extend `compass_from_towards.rs` with one assertion per
    direction.

### Caching / single-flighting

15. **Hub-line cache MUST cache `NotFound`.** `hub_lines_cached` stores an
    empty `Vec<LineRef>` on `TflError::NotFound` so a hub the live API
    genuinely doesn't expose (e.g. one of ~190 tube hubs whose detail
    endpoint we've never recorded a fixture for) is fetched once per
    process lifetime, not on every cold-warm. Transient errors (transport,
    rate-limited) MUST NOT be cached — a 429 must retry on the next warm.

16. **`stop_points_cached` single-flights concurrent refreshes.** A
    debounced search burst (200 ms typing → three keystrokes within a
    TTL-expiry window) without single-flighting issues 3× redundant
    per-mode + hub fan-outs. The async `tokio::sync::Mutex` field
    `stop_points_refresh` serialises: first caller does the work,
    subsequent callers await, re-check the cache, and return immediately.
    Hold the lock across the per-mode + hub fan-out so re-checkers see the
    freshly-stamped cache. Read-only callers (post-warm hits) never touch
    the lock.

17. **Hub fan-out dedupes by `hub_naptan_code` before parallel fetch.**
    `HUBKGX` is referenced by ~23 stations across tube + DLR + Elizabeth +
    Overground feeds; the naive enumerate-and-go fires 23 racers because
    `hub_lines_cached` only caches *after* the first fetch resolves. Build
    `stations_per_hub: HashMap<hub_id, Vec<usize>>` first — cuts a 757-fetch
    warm to 90 unique hubs.
    Test: `warm_stop_points_dedupes_hub_fetches_before_fan_out`.

18. **`search_stations` dedupes canonical entries that share a
    `hub_naptan_code`.** At Bank/Farringdon both `940GZZLUBNK` and
    `940GZZDLBNK` carry `hubNaptanCode: HUBBAN`. After hub-merge they also
    carry the same line union — two near-identical dropdown rows routing
    to the same arrivals. Keep one canonical per hub, prefix priority
    **`940GZZLU` (tube) > `940GZZDL` (DLR) > `910G` (OG / Elizabeth)** —
    the user's mental model maps a hub to its tube parent at every
    interchange we surface. Stations without a `hub_naptan_code`
    (Hampstead Heath, Belsize Park, single-mode stops) MUST NOT be
    deduped — no hub partner.

19. **`resolve_arrival_ids` and `allowed_line_ids_for` read
    `read_cache_any`, not `read_fresh_cache`.** The 15-min TTL controls
    when the cache *refreshes*, not when it stops being *useful*. Past
    TTL, `hub_naptan_code` and `lines` remain valid; the periodic task
    refreshes on its own schedule. Reading `read_fresh_cache` here loses
    hub-merge after expiry — Bank/Euston/Whitechapel siblings stop being
    fetched, and `line_ids = ["lioness"]` at Euston silently shows zero
    arrivals because the only data path that could return Lioness
    predictions (the OG sibling fetch) was bypassed.
    Test: `allowed_line_ids_for_serves_stale_cache_past_ttl`.

20. **Stop-points cache is stale-while-revalidate; refresh is
    out-of-band.** `stop_points_cached` returns whatever's cached (fresh or
    stale); only a *cold* cache blocks on the network. A periodic task in
    `lib.rs::run` calls `client.refresh_stop_points_cache()` every ~14 min
    (just under `STOP_POINTS_TTL`) — it runs the single-flighted fan-out
    via `refresh_stop_points_inner(force = true)`. The cold path uses the
    same inner with `force = false` so a concurrent refresher's stamp
    short-circuits us. Without this decoupling, a debounced search burst
    at the TTL boundary blocks ~1–3 s on a redundant fan-out.
    Tests: `search_stations_does_not_refetch_when_cache_is_stale_but_present`,
    `refresh_stop_points_cache_forces_refetch_even_when_fresh`.

21. **Per-mode stop-points fetch retries on transient errors.** The 4-mode
    fan-out retries each mode up to `STOP_POINTS_FETCH_ATTEMPTS` (4) with
    exponential backoff (`STOP_POINTS_FETCH_BACKOFF`: 500ms / 1.5s / 4.5s)
    on `RateLimited` / `Transport` / `Http`. `NotFound`, `Parse`, `ParseAt`
    are terminal. Without retries, a single 429 mid-burst (anonymous 50
    req/min budget — common on iOS over cellular without an `app_key`)
    leaves a whole mode missing for the full 14-minute window. User
    symptom: "tube doesn't appear in search but DLR does".

### Frontend

22. **The `line_ids` chip filter is a frontend-only display mask.**
    `Board.svelte`'s `linesGrouped` skips arrivals whose `line_id` isn't in
    the user's `lineIds` prop; backend `apply_filters`
    (`crates/tfl-board/src/filter.rs`) does NOT filter by `line_ids` and
    MUST keep handing the full set through. Why: chip toggles update the
    visible board in a frame, no waiting ~30 s for the next tick. Backend
    filters that DO stay backend-side: (a) `directions` (toggled
    infrequently), (b) `drop_arrivals_for_lines_not_serving` (defensive
    integrity, independent of preference).

## Test harness — the rules

**Tests are not optional for this pipeline.** Visual smoke is not enough;
production timing differs from any single dev interaction.

- Use `fixture_state_with_stream(seed)` for ALL tests that assert anything
  about the cfg → stream pipeline. `fixture_state()` drops the receiver and
  `cfg_tx.send` returns `Err(NoReceivers)` swallowed by `let _ = ...`.
- Stream timing tests use `#[tokio::test(start_paused = true)]` plus
  `tokio::time::advance` (requires the `tokio/test-util` dev-feature in
  `src-tauri/Cargo.toml`). Real-clock tests are non-deterministic.
- Use `tokio::time::timeout(Duration::from_millis(50), stream.next())`
  with `.is_err()` to assert "no emit fires". Polling once and hoping is
  racy.
- Use `tokio::time::Instant::now()` + `.elapsed()` to bound how long an
  expected emit took — only way to catch "station changed but stream
  waited 30 s" under `start_paused`, where a bug lets the paused clock
  auto-advance the full poll interval.
- Frontend has parallel coverage in `web/src/lib/__tests__/dom/` using
  happy-dom. Add a DOM test for any UI-visible state transition.
- Verify a new test would fail without your fix — revert locally and watch
  it go red. "I added a test" with the wrong fixture is meaningless.

## Required regression coverage

Commit-blocking: `cargo test --workspace` and `cd web && npm test` must be
green. Pay particular attention to these files; add new regression tests
to whichever fits when you discover a new failure mode.

| File                                              | Covers                                                                                                                              |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/commands.rs`                       | `save_config` → stream pipeline (#1–3); display-mode lock (#8); board-size validation (#9); favorites; legacy line-id migration (#14) |
| `crates/tfl-board/src/service.rs`                 | Stream timing (immediate refresh on station change, no refresh on filter change, last_ok lifecycle); terminus filter (#24); line-not-served filter (#10) |
| `crates/tfl-board/tests/board_service_tests.rs`   | End-to-end fixture refresh; phantom-line + Overground regression coverage; `build_board_preserves_distinct_arrivals_with_same_id`   |
| `crates/tfl-board/src/filter.rs`                  | `apply_filters_does_not_filter_by_line_id` — frontend-only mask contract (#22)                                                      |
| `crates/tfl-client/src/client_tests.rs`           | Multi-mode fan-out (#12); SWR caching (#19–20); hub-fan-out dedupe (#17); search-stations whitelist (#13); per-mode retry (#21); subset client; multi-mode interchange dedupe (#18) |
| `crates/tfl-client/tests/http_retry.rs`           | `app_key` query-param wiring; 429 cooldown                                                                                          |
| `crates/tfl-domain/tests/compass_from_towards.rs` | Per-line `towards` → compass mapping (#23)                                                                                          |
| `web/src/lib/__tests__/dom/`                      | Board store latest-wins (#7), debounce-coalesce, line-group rendering + line-stripe correctness (#11), OG line colours, settings UI (display-mode, favorites, OG chips), adaptive resize, board-error event, line-id display-filter (#22) |

## Verifying a stream/config change

1. `cargo test --workspace` — green.
2. `cd web && npm test` — green.
3. `cargo clippy --workspace --all-targets -- -D warnings` — clean.
4. **Manual smoke** (don't skip for stream/config changes — `cargo tauri dev`):
   - Switch station to a multi-line station (King's Cross). Board updates
     within ~1 s. Tail log for 429s and `stream tick recovered`.
   - Rapid-toggle 6+ chips. No flicker, no stream respawn.
   - A → B → A. Board emits each in turn (B briefly visible, then A).
   - `poll_seconds` 30 → 60 via slider. Next tick fires ~60 s later. No
     stream restart in logs.

For **display-mode** changes (`apply_display_mode_effects` /
`save_display_mode` / `display_mode` lock):

- Window → menubar: dock icon disappears, window hides, tray appears, tray
  click shows popover at 380×560.
- Menubar → window: tray gone, dock back, window 980×720 centered with the
  LED title bar (no native chrome).
- Toggle 5× rapidly: no crashes, no duplicate trays, final state matches
  last toggle.
- Mid-tick mode swap: `board://updated` keeps flowing (mode swap touches
  no stream state).
- Mode swap → station change still works (no lock starvation, no leaked
  Arc cycle).

For **adaptive resize / line-grouped layout** (`apply_board_size` /
`Board.svelte::pickBoardSize` / `linesGrouped`). Cycle through:

- **1-line** (Belsize Park, Stockwell): menubar 380×520, window 700×560.
  One LINE header, two direction columns under it.
- **2-line** (Oxford Circus, Green Park): menubar 380×620, window 980×680.
- **3-line** (Tottenham Court Road, Bank): menubar 380×720, window 1200×760.
- **4+ line** (Baker Street, King's Cross): menubar 380×800, window
  1200×880. **Every line stripe must match its line header** — no Bakerloo
  orange under Metropolitan, no Jubilee silver under Bakerloo. Mixed
  stripes = invariant #11 regressed.
- Back to 1-line: single resize step, no flicker, no intermediate sizes.
- Sitting on one station 60 s (two ticks) MUST NOT issue extra resize
  requests — the renderer-side `lastSizeKey` dedupe is what protects the
  Cocoa main-thread dispatch.

## External consumers of `crates/tfl-*`

`tfl-domain`, `tfl-client`, `tfl-board` are consumed by
[`argen/tubbie-ios`](https://github.com/argen/tubbie-ios) via a SHA-pinned
git submodule. Their public surface is a contract — see
`docs/ADR/crates-as-public-contract.md`. Breaking changes need a paired PR
+ submodule bump in tubbie-ios. Internal refactors are unaffected.

## Things that look correct in isolation but break the integration

- A passing `BoardService::stream` test does NOT prove `save_config`
  reaches the stream — those tests use a `watch::channel` directly, not
  `AppState.cfg_tx`. Use `fixture_state_with_stream`.
- `fixture_state()` quietly accepts `cfg_tx.send` calls because of
  `let _ = ...`, so end-to-end command tests using it can pass while the
  pipeline is fundamentally broken. Never use it for `save_config` cases.
- A clean `cargo test` does NOT mean the user-visible UX is correct. Run
  the manual smoke for any stream/config change.

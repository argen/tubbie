# CLAUDE.md — agent guidance for tubbie

> Read this BEFORE editing the stream / config / settings pipeline. Every
> rule here is here because something broke. Add to it when something
> breaks again.

## What tubbie is

A Tauri v2 desktop app that shows live TfL arrivals. Rust backend in
`src-tauri/` and `crates/tfl-*`, SvelteKit + Svelte 5 runes frontend in
`web/`. See [`docs/architecture.md`](docs/architecture.md) for the crate
graph and module boundaries.

## The pipeline you'll keep breaking

This is the only pipeline that matters for "did the board update?":

```
Settings UI                  Tauri command      AppState        Stream task          board://updated
──────────                   ─────────────      ────────        ───────────          ───────────────
toggleLine() / chip / etc.   save_config        cfg_tx.send  →  cfg_rx.changed()  →  app.emit(...)
        │                       │                                   │
   persistDebounced (400 ms)    save to store                  refresh + last_ok logic
```

If you change `save_config_inner`, `BoardService::stream`, `AppState`,
the `watch::Sender`/`Receiver` wiring, or the spawn_stream_task respawn
path: you are touching this pipeline. **Run the integration tests at the
bottom of this doc, not just the unit tests.**

## Invariants (don't violate without a test that proves the new contract)

1. **`save_config_inner` MUST publish to a live `watch::Receiver`.**
   `state.cfg_tx.send(cfg)` fails silently when no receiver exists
   (`let _ = ...`). Production has a receiver in the stream task; the
   test fixture `fixture_state()` does NOT — it drops `_cfg_rx`. Use
   `fixture_state_with_stream(...)` for any test that asserts effects of
   `save_config` on the stream.

2. **A `station_id` change MUST trigger an immediate refresh.** The
   "cheap CfgChanged" semantic — apply side-effects, wait for the next
   tick — is correct for filters/theme/poll_seconds but wrong for
   station picks. Users won't tolerate up to `poll_seconds` (30 s) of
   stale data after a deliberate click. The
   `stream_refreshes_immediately_on_station_id_change` test guards this
   in `crates/tfl-board/src/service.rs`.

3. **Filter / theme / `directions` changes MUST NOT trigger a fresh
   fetch.** That was the whole point of the watch-channel refactor:
   chip-toggle bursts must coalesce. The
   `save_config_filter_change_does_not_force_immediate_refresh` test
   asserts the no-fetch behaviour. **Note:** `line_ids` was previously
   on this list but moved to frontend-only display filtering (invariant
   #22) — backend `apply_filters` no longer touches it, so the
   "MUST NOT refetch" rule is moot for that field today.

4. **The stream is infinite. The consumer MUST NOT `break` on `Err`.**
   `BoardService::stream` is contractually infinite — on fetch failure
   with no `last_ok` it emits the error and keeps polling. Breaking at
   the consumer (`spawn_stream_task` in `lib.rs`) makes the watcher
   respawn the task every 2 s, hammering TfL through any 429 cooldown.
   Log once per error streak; let `poll_seconds` throttle retries.

5. **`AppState` and the stream task MUST share one `Arc<TflClient>`.**
   That is what shares caches (`stop_points_cache`, `hub_children_cache`,
   `line_status_cache`), the connection pool, and the 429 cooldown gate
   process-wide. A fresh client on every spawn means a fresh 16 MB
   `/StopPoint/Mode/tube` warm.

6. **The initial config MUST be loaded synchronously before the stream
   spawns.** `tauri::async_runtime::block_on(config_store.load_config())`
   is correct here — the in-memory store load cannot deadlock and avoids
   the race where the stream's first tick would refresh the default
   station before an async loader could publish the saved one.

7. **Generated_at on `Board` is the latest-wins key in the frontend.**
   `applyBoard` in `web/src/lib/stores/board.ts` ignores any board whose
   `generated_at` is `<=` the current. If you ever return a board with a
   stale clock (e.g. by re-emitting `last_ok` without bumping
   `generated_at`), the UI silently freezes.

8. **Cocoa display-mode side-effects MUST run on the macOS main thread.**
   `apply_display_mode_effects_sync` in `lib.rs` calls
   `set_activation_policy`, `remove_tray_by_id` (whose returned
   `TrayIcon::Drop` calls `NSStatusBar::removeStatusItem`),
   `set_decorations`, `set_size`, etc. Each of these asserts a
   main-thread barrier in Cocoa (`BSServiceMainRunLoopQueue::
   assertBarrierOnQueue`) and crashes the process with `EXC_BREAKPOINT`
   if called from a Tokio worker. Tauri commands run on Tokio. The
   public async `apply_display_mode_effects` wrapper hops to the main
   thread via `run_on_main_thread` + a oneshot — always call the async
   version from any non-`setup` caller. Setup itself runs on the main
   thread and uses the sync version directly (the async one would
   deadlock — `run_on_main_thread` queues a user event that can only be
   drained after setup returns).

9. **`apply_board_size` MUST hop to the macOS main thread too.**
   `WebviewWindow::set_size` reaches `NSWindow::setFrame:display:` —
   same Cocoa assertion as #8. The public async
   `apply_board_size_effects` wrapper exists for exactly this reason;
   the Tauri `apply_board_size` command in `commands.rs` calls it after
   `validate_board_size`. Validation runs *before* dispatch so a buggy
   renderer (NaN, infinity, out-of-range) never reaches Cocoa. The
   renderer dedupes per tier (`lastSizeKey` in `Board.svelte`), so
   per-tick board updates don't pound the main thread when nothing
   about the layout changed. Don't bypass the dedupe by re-issuing the
   resize from any other component; the Board owns it.

10. **`BoardService::refresh` MUST drop arrivals whose `line_id` is
    not served by the queried station.** TfL occasionally surfaces
    predictions under stop-points that don't physically serve that
    line (the most likely path is hub-merge in
    `TflClient::get_arrivals`, where a sibling stop-point's prediction
    leaks under the parent). The defensive filter
    `drop_arrivals_for_lines_not_serving` in `crates/tfl-board/src/service.rs`
    runs after `apply_filters` and uses
    `TflClient::allowed_line_ids_for(station_id)` as the source of
    truth — that method projects the hub-aware `Station.lines` field
    that `stop_points_cached` already populates with the union across
    every hub child. Fail-open semantics: when the stop-points cache is
    cold (`read_fresh_cache` returns `None`), the allowed set is empty
    and the filter is skipped — dropping legitimate arrivals because
    of a cold cache is worse UX than letting one phantom through until
    the cache warms (production hits this only for the very first
    refresh; the warm task in `lib.rs::run` populates the cache after
    the first board emit). A disallowed arrival emits one warning per
    `(station, line)` pair per refresh on stderr with the
    `[tfl-board]` prefix. Don't silence it — the warning is the only
    signal we have that the upstream data shape is drifting.

11. **Line-grouped UI MUST re-bucket arrivals per-line, not per-platform.**
    The Rust backend groups arrivals by `Direction` — `Board.platforms[]`
    is at most seven entries (Northbound … Unknown) and a single
    direction bucket explicitly mixes lines (King's Cross "Westbound"
    has hammersmith-city + metropolitan; Baker Street southbound has
    Bakerloo + Jubilee on the shared bay platforms — see
    `crates/tfl-board/tests/board_service_tests.rs::refresh_groups_by_direction`).
    The frontend's `linesGrouped` derivation in `Board.svelte` walks
    every arrival, buckets by `line_id`, then by `direction` inside
    each line. Grouping by `Platform.arrivals[0].line_id` (the previous
    naive approach) silently mis-colours every minority-line train;
    don't reintroduce it. The synthetic platform handed to
    `PlatformColumn` carries `name = direction.label` and the line+
    direction-filtered arrivals — `PlatformColumn`'s dedupe key
    `${line_id}|${platform_name}|${expected_arrival}` stays unique
    because `platform_name` differs across the physical platforms the
    backend merged.

12. **Multi-mode caches MUST cover tube + dlr + overground + elizabeth-line,
    fetched per-mode and merged.** TfL's `/StopPoint/Mode/{mode}` and
    `/Line/Mode/{mode}/Status` endpoints accept a single mode each.
    Comma-separated multi-mode strings are forbidden by the
    `[a-zA-Z0-9_-]{1,64}` validator in `crates/tfl-client/src/fixture.rs`,
    and widening that regex is the path-traversal regression that
    `architecture.md` warns against. The stop-points cache is one merged
    `Vec<Station>` keyed by `Station.id` with line-list union on
    collision; the line-status cache is one merged `Vec<TflLine>`. Both
    fan out across `tfl_client::SUPPORTED_MODES` in parallel via
    `futures::future::join_all` on cold-warm. A subset client for
    memory-constrained downstream consumers (the iOS shell) goes
    through `TflClient::with_modes(http, &[&str])`. Adding a mode
    requires (a) extending `SUPPORTED_MODES`, (b) recording matching
    `fixtures/{stop-points,line-status}/{mode}.json`, (c) extending
    `is_supported_line_id` in `tfl-domain`, and (d) extending the
    canonical-id prefix whitelist in #13 if its NaPTAN scheme differs.

13. **Station search MUST use a NaPTAN canonical-prefix whitelist, not a
    `modes` list.** Allowed prefixes are `940GZZLU` (tube canonical),
    `940GZZDL` (DLR canonical), and `910G` filtered to stops whose
    `modes` include `overground` or `elizabeth-line` — the 910G NaPTAN
    range overlaps with National Rail-only operators (Gatwick Express,
    Thameslink, Southern, …) which we don't surface. `9400ZZLU*`,
    `4900*`, `2100*`, and `HUB*` MUST stay excluded — they're
    platform-level children, NaPTAN bus-stop-at-station records, and
    multi-mode aggregators with no stable arrivals id. Adding a mode
    with a different prefix scheme requires extending the whitelist
    and adding a regression test of the form
    `search_stations_includes_<mode>_only_station` against a real
    fixture entry.

14. **Legacy `"london-overground"` config ids MUST be migrated on load.**
    TfL stopped emitting predictions under the legacy id when the six
    named lines launched in November 2024. `load_config_inner` calls
    `migrate_legacy_line_ids` in `commands.rs`, which expands any
    `"london-overground"` entry in `BoardConfig.line_ids` into the six
    successor ids `[liberty, lioness, mildmay, suffragette, weaver,
    windrush]` at the same position, deduping against existing entries.
    Idempotent. Keep `"london-overground"` accepted by
    `is_supported_line_id` so historical fixtures still parse. The
    migration is the only thing standing between an upgrading user and
    a silently-empty Overground board.

15. **Hub-line cache MUST cache `NotFound` results.** `hub_lines_cached`
    in `client.rs` stores an empty `Vec<LineRef>` on `TflError::NotFound`
    so a hub the live API genuinely doesn't expose (e.g. one of the
    ~190 tube hubs whose detail endpoint we've never recorded a fixture
    for) is fetched once per process lifetime, not on every cold-warm.
    Transient errors (transport, rate-limited) MUST NOT be cached — a
    429 must retry on the next warm. Without this, a single warm cycle
    fans out 190+ hub fetches; with it, it's bounded by the count of
    hubs whose data has ever resolved.

16. **`stop_points_cached` MUST single-flight concurrent refreshes.**
    The sync `Mutex<Option<…>>` cache check is fast, but releasing the
    lock and starting an async fan-out lets a debounced search burst
    (200 ms typing → three keystrokes within a TTL-expiry window) each
    see an empty cache and fire its own full per-mode + hub fan-out —
    a 3× redundant TfL workload. The async `tokio::sync::Mutex` field
    `stop_points_refresh` serialises refreshes: the first caller does
    the work; subsequent callers await, then re-check the cache and
    return immediately. Hold the lock across the per-mode + hub
    fan-out so the second caller's re-check sees the freshly-stamped
    cache. Read-only callers (post-warm cache hits) never touch the
    refresh lock.

17. **Hub fan-out MUST be deduped by `hub_naptan_code` before the
    parallel fetch.** A single hub like `HUBKGX` is referenced by ~23
    stations across the tube + DLR + Elizabeth + Overground feeds.
    The naive `iter().enumerate()` approach fires one job per station,
    but `hub_lines_cached` only caches AFTER its first fetch resolves —
    so 23 racers all see an empty cache and each issue their own HTTP
    request. Building `stations_per_hub: HashMap<hub_id, Vec<usize>>`
    before the fan-out cuts a 757-fetch warm-time burst to 90 unique
    hubs. Guarded by `warm_stop_points_dedupes_hub_fetches_before_fan_out`.

18. **`search_stations` MUST dedupe canonical entries that share a
    `hub_naptan_code`.** At multi-mode interchanges (Bank,
    Farringdon, …) the per-mode `/StopPoint/Mode/{mode}` feeds each
    return their own canonical entry — `940GZZLUBNK` and `940GZZDLBNK`
    both have `hubNaptanCode: HUBBAN` because they're the same physical
    station. After hub-merge they also carry the same union of lines,
    so the dropdown would show two near-identical rows that route to
    the same arrivals. Keep one canonical entry per hub code, with the
    prefix priority **`940GZZLU` (tube) > `940GZZDL` (DLR) > `910G`
    (Overground / Elizabeth)** — the user's mental model maps a hub to
    its tube parent at every interchange we surface today. Stations
    without a `hub_naptan_code` (Hampstead Heath, Belsize Park, single-
    mode stops) MUST NOT be deduped — they have no hub partner.

19. **`resolve_arrival_ids` and `allowed_line_ids_for` MUST read from
    `read_cache_any`, not `read_fresh_cache`.** The 15-min stop-points
    TTL controls when the cache *refreshes*, not when it stops being
    *useful*. Past the TTL, a cached entry's `hub_naptan_code` and
    `lines` fields remain valid (TfL station metadata changes
    infrequently); the periodic background task in `lib.rs::run` will
    refresh on its own schedule. If the stream's arrivals path uses
    `read_fresh_cache` instead, the first tick after expiry loses
    hub-merge — Bank/Euston/Whitechapel siblings are never fetched and
    a user with the chip filter set (e.g. `line_ids = ["lioness"]` at
    Euston) silently sees zero arrivals because the only data path
    that could return Lioness predictions (the OG sibling fetch) was
    bypassed. Guarded by
    `allowed_line_ids_for_serves_stale_cache_past_ttl`.

20. **Stop-points cache is stale-while-revalidate; refresh runs out-of-
    band.** `stop_points_cached` returns whatever's currently cached
    (fresh or stale) and only blocks on the network for a *cold* cache
    (first call ever, never warmed). The TTL no longer gates user-
    facing reads at all. A periodic task spawned in `lib.rs::run` calls
    `client.refresh_stop_points_cache()` every ~14 minutes (just under
    `STOP_POINTS_TTL`) to keep the cache fresh. That public refresh
    method runs the single-flighted fan-out + hub-merge unconditionally
    via `refresh_stop_points_inner(force = true)`. The cold-cache path
    still goes through the same inner with `force = false` so a
    concurrent refresher's stamp short-circuits us. Without this
    decoupling, a debounced search burst at the TTL boundary blocks for
    ~1–3 s on a redundant fan-out the user didn't ask for, and the
    multi-mode hub-merge starves the chip filter at hub stations
    until the next manual search triggers a refresh. Guarded by
    `search_stations_does_not_refetch_when_cache_is_stale_but_present`
    and `refresh_stop_points_cache_forces_refetch_even_when_fresh`.

21. **Per-mode stop-points fetch MUST retry on transient errors.**
    The 4-mode parallel fan-out in `refresh_stop_points_inner` retries
    each mode up to `STOP_POINTS_FETCH_ATTEMPTS` times (currently 4)
    with exponential backoff (`STOP_POINTS_FETCH_BACKOFF`: 500 ms /
    1.5 s / 4.5 s) on `RateLimited`, `Transport`, and `Http` errors.
    `NotFound`, `Parse`, and `ParseAt` are terminal — retrying won't
    fix bad JSON or a missing endpoint. Without retries, a single 429
    mid-burst (common on the anonymous 50 req/min budget — observed
    on iOS over cellular without an `app_key`) leaves a whole mode
    missing from the cache for the full 14-minute periodic-refresh
    window. The user's symptom: "tube doesn't appear in search but
    DLR does" until the next periodic refresh. Guarded by
    `warm_retries_per_mode_on_transient_failure` and
    `warm_does_not_retry_on_terminal_errors`.

22. **The user-facing `line_ids` chip filter is a frontend-only
    display mask.** `Board.svelte`'s `linesGrouped` derivation skips
    arrivals whose `line_id` is not in the user's `lineIds` prop;
    backend `apply_filters` (`crates/tfl-board/src/filter.rs`) does
    NOT filter by `line_ids` and MUST keep handing the full set
    through. Why: chip toggles update the visible board in a frame —
    no waiting for the next ~30 s periodic stream tick to re-emit a
    backend-filtered payload. Two related filters that DO stay in the
    backend: (a) `directions` (still in `apply_filters`; toggled
    infrequently enough that the tick latency is invisible), (b)
    `drop_arrivals_for_lines_not_serving` (the per-station defensive
    integrity filter — independent of user preference, drops phantom
    predictions). Guarded by
    `crates/tfl-board/src/filter.rs::apply_filters_does_not_filter_by_line_id`,
    `src-tauri/src/commands.rs::save_config_then_get_board_applies_station_but_does_not_filter_lines`,
    and `web/src/lib/__tests__/dom/board-line-id-display-filter.dom.test.ts`.

## Test harness — the rules

**Tests are not optional for this pipeline.** Visual smoke testing is
not enough; the production timing differs from any single dev
interaction.

- Use `fixture_state_with_stream(seed)` from `src-tauri/src/commands.rs`
  for ALL tests that assert anything about the cfg → stream pipeline.
  `fixture_state()` drops the receiver and any `cfg_tx.send` returns
  `Err(NoReceivers)` swallowed by `let _ = ...`. The dedicated test
  `fixture_state_with_stream_keeps_receiver_alive` exists to catch
  regressions of the helper itself.
- Stream timing tests use `#[tokio::test(start_paused = true)]` plus
  `tokio::time::advance` (requires the `tokio/test-util` dev-feature in
  `src-tauri/Cargo.toml`). Real-clock tests are non-deterministic.
- Use `tokio::time::timeout(Duration::from_millis(50), stream.next())`
  with `.is_err()` to assert "no emit fires" cases. Polling once and
  hoping it doesn't fire is racy.
- Use `tokio::time::Instant::now()` + `.elapsed()` to bound how long an
  expected emit took — that's the only way to catch "station changed
  but stream waited 30 s" regressions under `start_paused`, where a
  bug lets the paused clock auto-advance the full poll interval.
- The frontend has parallel coverage in `web/src/lib/__tests__/dom/`
  using happy-dom. Add a DOM test for any UI-visible state transition
  you change.

## Required regression tests (commit-blocking)

These tests must stay green or you're shipping a regression:

| File                                                          | Test                                                     | Guards                          |
| ------------------------------------------------------------- | -------------------------------------------------------- | ------------------------------- |
| `src-tauri/src/commands.rs`                                   | `save_config_publishes_station_change_to_running_stream` | save → cfg_tx → stream → emit   |
| `src-tauri/src/commands.rs`                                   | `save_config_a_then_b_then_a_emits_each_in_order`        | back-and-forth station picks    |
| `src-tauri/src/commands.rs`                                   | `save_config_filter_change_does_not_force_immediate_refresh` | no fetch on filter toggle    |
| `src-tauri/src/commands.rs`                                   | `fixture_state_with_stream_keeps_receiver_alive`         | the test harness itself         |
| `crates/tfl-board/src/service.rs`                             | `stream_refreshes_immediately_on_station_id_change`      | station change UX               |
| `crates/tfl-board/src/service.rs`                             | `stream_picks_up_directions_change_without_restart`      | watch-channel still wired       |
| `crates/tfl-board/src/service.rs`                             | `stream_rebuilds_interval_when_poll_seconds_changes`     | poll_seconds applies live       |
| `crates/tfl-board/src/service.rs`                             | `stream_drops_last_ok_when_station_id_changes`           | no cross-station data leak      |
| `crates/tfl-board/tests/board_service_tests.rs`               | `stream_terminates_after_fatal_error_no_last_ok`         | infinite-stream contract        |
| `crates/tfl-board/tests/board_service_tests.rs`               | `every_fixture_arrival_carries_a_line_id_known_to_the_station` | end-to-end: refresh through fixture → no out-of-set line_id reaches the board (uses `allowed_line_ids_for` as source of truth) |
| `crates/tfl-board/tests/board_service_tests.rs`               | `refresh_drops_arrivals_for_lines_not_serving_the_station` | injects a phantom Bakerloo arrival at Belsize Park; defensive filter must drop it while keeping the legitimate Northern arrival |
| `crates/tfl-client/tests/http_retry.rs`                       | `with_app_key_appends_query_param`                       | app_key reaches the wire        |
| `crates/tfl-client/tests/http_retry.rs`                       | `concurrent_calls_share_429_cooldown`                    | rate-limit gate                 |
| `web/src/lib/__tests__/dom/board-store-seed.dom.test.ts`      | latest-wins by `generated_at`                            | board store regression check    |
| `web/src/lib/__tests__/dom/settings-debounce-persist.dom.test.ts` | chip-burst coalesces                                     | rate-limit-blowing chip clicks  |
| `web/src/lib/__tests__/dom/PlatformColumn-duplicate-ids.dom.test.ts` | renders all distinct trains when ids collide / dedupes on full composite | TfL non-unique `Arrival.id` + defensive each-key dedup |
| `crates/tfl-board/src/service.rs`                             | `build_board_preserves_distinct_arrivals_with_same_id`  | no silent dedup-by-id           |
| `web/src/lib/__tests__/dom/board-error-event.dom.test.ts`     | board://error → boardError when no board; preserves existing | stream-error propagation contract |
| `src-tauri/src/commands.rs`                                   | `save_display_mode_updates_live_state_lock`              | live display_mode lock matches saved value |
| `src-tauri/src/commands.rs`                                   | `save_display_mode_invalid_value_does_not_mutate_state`  | rejected save leaves runtime + store untouched |
| `src-tauri/src/commands.rs`                                   | `save_display_mode_idempotent_when_mode_unchanged`       | re-saving current mode is a no-op |
| `web/src/lib/__tests__/dom/settings-display-mode-live.dom.test.ts` | radio reflects $displayMode; save updates store; rejection rolls back; same-mode click is a no-op | live-toggle UI contract |
| `src-tauri/src/commands.rs`                                   | `validate_board_size_accepts_renderer_preset_table`      | each preset tier passes validation; bounds drift guard |
| `src-tauri/src/commands.rs`                                   | `validate_board_size_rejects_out_of_range`               | buggy renderer can't crash Cocoa via degenerate `set_size` |
| `src-tauri/src/commands.rs`                                   | `validate_board_size_rejects_non_finite`                 | NaN / infinity refused before reaching `NSWindow::setFrame:` |
| `web/src/lib/__tests__/dom/board-line-groups.dom.test.ts`     | mixed-line direction bucket splits per-line; 5-line interchange renders 5 groups; directions sort by compass order; multi-platform merges within (line, direction) | line-grouped layout contract — covers Baker Street / King's Cross corner cases |
| `web/src/lib/__tests__/dom/board-resize-request.dom.test.ts`  | each preset tier (1 / 2 / 3 / 4+ lines × menubar/window) triggers correct dims; same board re-render dedupes; switching stations re-fires | adaptive resize contract |
| `crates/tfl-client/src/client_tests.rs`                       | `get_line_status_overground_returns_status`              | overground line statuses reach the ticker |
| `crates/tfl-client/src/client_tests.rs`                       | `get_line_status_dlr_returns_status`                     | dlr line statuses reach the ticker |
| `crates/tfl-client/src/client_tests.rs`                       | `get_line_status_elizabeth_returns_status`               | elizabeth line statuses reach the ticker |
| `crates/tfl-client/src/client_tests.rs`                       | `client_with_subset_modes_only_fetches_those_modes`      | `with_modes` honoured by both line-status and stop-points fan-outs |
| `crates/tfl-client/src/client_tests.rs`                       | `stop_points_cache_includes_overground_dlr_elizabeth_stations` | multi-mode cache fan-out |
| `crates/tfl-client/src/client_tests.rs`                       | `search_stations_returns_overground_only_station`        | overground-only stops reachable in search |
| `crates/tfl-client/src/client_tests.rs`                       | `search_stations_includes_dlr_only_station`              | latent DLR-only-station bug fixed |
| `crates/tfl-client/src/client_tests.rs`                       | `search_stations_excludes_national_rail_only_910g_stations` | 910G NaPTAN range mode-filtered |
| `crates/tfl-client/src/client_tests.rs`                       | `search_stations_excludes_platform_children_and_hubs`    | prefix whitelist still rejects 9400/4900/2100/HUB |
| `crates/tfl-client/src/client_tests.rs`                       | `stop_points_cache_dedupes_station_id_across_modes_and_unions_lines` | cross-mode merge contract |
| `crates/tfl-client/src/client_tests.rs`                       | `search_whitechapel_includes_elizabeth_and_windrush_chips` | hub-merge brings Overground + Elizabeth siblings into a tube parent |
| `crates/tfl-board/tests/board_service_tests.rs`               | `refresh_emits_legitimate_overground_arrivals_at_overground_station` | end-to-end Overground at Hackney Central |
| `crates/tfl-board/tests/board_service_tests.rs`               | `refresh_drops_phantom_overground_arrival_at_tube_only_station` | defensive filter rejects Mildmay phantom at BZP |
| `src-tauri/src/commands.rs`                                   | `migrate_legacy_line_ids_rewrites_legacy_overground`     | one-shot rewrite of legacy id |
| `src-tauri/src/commands.rs`                                   | `migrate_legacy_line_ids_is_idempotent`                  | re-running on migrated config is a no-op |
| `src-tauri/src/commands.rs`                                   | `migrate_legacy_line_ids_dedupes_against_existing_named_ids` | merge preserves existing ids and positions |
| `src-tauri/src/commands.rs`                                   | `load_config_migrates_legacy_london_overground_id`       | end-to-end load path runs the migration |
| `web/src/lib/__tests__/dom/settings-overground-chips.dom.test.ts` | all 6 named OG chips + DLR appear; only station-served ones enabled; toggling Mildmay writes into line_ids | settings UI contract for the multi-mode rollout |
| `web/src/lib/__tests__/dom/board-line-groups.dom.test.ts`     | Mildmay + Windrush at a multi-line OG hub: per-line stripe matches per-line group | line-stripe correctness invariant #11 holds for OG line ids |
| `web/src/lib/__tests__/dom/ArrivalRow.dom.test.ts`            | each of the 6 named OG ids resolves to its own `--line-{id}` CSS variable | OG colours aren't aliased to a generic Overground orange |
| `crates/tfl-client/src/client_tests.rs`                       | `search_stations_does_not_refetch_when_cache_is_stale_but_present` | SWR — search never blocks on TTL refresh |
| `crates/tfl-client/src/client_tests.rs`                       | `refresh_stop_points_cache_forces_refetch_even_when_fresh` | periodic background refresh actually does the work |
| `crates/tfl-client/src/client_tests.rs`                       | `allowed_line_ids_for_serves_stale_cache_past_ttl` | hub lookup survives the TTL boundary |
| `crates/tfl-client/src/client_tests.rs`                       | `warm_stop_points_dedupes_hub_fetches_before_fan_out` | hub fan-out deduped (757 → 90) |
| `crates/tfl-client/src/client_tests.rs`                       | `search_dedupes_multi_mode_interchange_to_one_row` | Bank/Farringdon collapse to single canonical |
| `crates/tfl-client/src/client_tests.rs`                       | `warm_retries_per_mode_on_transient_failure` | per-mode retry on 429 / transport blip |
| `crates/tfl-client/src/client_tests.rs`                       | `warm_does_not_retry_on_terminal_errors` | NotFound / Parse never retried |
| `crates/tfl-board/src/filter.rs`                              | `apply_filters_does_not_filter_by_line_id` | line_ids is a frontend display mask, not a backend filter |
| `src-tauri/src/commands.rs`                                   | `save_config_then_get_board_applies_station_but_does_not_filter_lines` | end-to-end: backend hands full set through; chip filter is display-only |
| `web/src/lib/__tests__/dom/board-line-id-display-filter.dom.test.ts` | lineIds prop masks line groups in `linesGrouped`; empty = show all | frontend chip filter contract |

If you add a new failure mode, add a test row here.

## How to actually verify a stream/config change

Before reporting work as complete:

1. `cargo test --workspace` — must be green. Pay special attention to
   the tests in the table above.
2. `cd web && npm test` — must be green.
3. `cargo clippy --workspace --all-targets -- -D warnings` —
   no new warnings.
4. **Manual smoke (don't skip this for stream/config changes):**
   - `cargo tauri dev` to launch.
   - Open Settings, switch station from default to a different
     multi-line station (e.g. King's Cross). Click Back. Board MUST
     show the new station within ~1 s. Tail the dev log for
     `stream tick recovered` or repeated 429s.
   - In Settings, rapid-toggle 6+ chips. Board MUST NOT flicker;
     log MUST NOT show stream respawn.
   - Force a station swap then return: A → B → A. Board emits each
     in turn (B briefly visible, then A).
   - Drop poll_seconds via slider 30 → 60. Next tick fires ~60 s
     later. No stream restart in logs.
   - **Display-mode live toggle** (only for `apply_display_mode_effects`
     / `save_display_mode` / `display_mode` lock changes):
     - Launch in window mode. Open Settings → switch to "Menu bar
       popover". Within ~1 s: dock icon disappears, window hides, tray
       icon appears in the menu bar. Left-click tray → popover shows
       under the icon at 380×560.
     - Switch back via Settings → "Floating window". Within ~1 s: tray
       icon disappears, dock icon reappears, window shows at 980×720
       centered with the LED title bar (no native chrome).
     - Toggle 5× rapidly. No crashes, no duplicate trays, final state
       matches the last toggle.
     - Switch mode while the stream is mid-tick. `board://updated`
       MUST keep flowing — the mode swap touches no stream state.
     - Switch mode, then change station. `save_config` still works
       (no lock starvation, no leaked Arc cycle).
   - **Adaptive board resize + line-grouped layout** (only for
     `apply_board_size` / `Board.svelte::pickBoardSize` /
     `linesGrouped` changes). Cycle through this set of stations to
     cover the line-count tiers and the multi-line corner cases:
     - **1-line** (Belsize Park, Stockwell): menubar 380×520, window
       700×560. One LINE header, two direction columns under it.
     - **2-line** (Oxford Circus, Green Park): menubar 380×620, window
       980×680. Two LINE headers stacked, two directions each.
     - **3-line** (Tottenham Court Road, Bank): menubar 380×720, window
       1200×760. Three LINE headers stacked.
     - **4+ line** (Baker Street — Metropolitan / Bakerloo / Circle /
       H&C / Jubilee — or King's Cross): menubar 380×800, window
       1200×880. **Critical: every line stripe on every row must
       match its line header** — no Bakerloo orange stripe under the
       Metropolitan group, no Jubilee silver stripe under Bakerloo.
       This is the line-grouping correctness check. If you see mixed
       stripes, the per-arrival grouping (invariant #10) regressed.
     - Switch back to a 1-line station. Window shrinks/popover shrinks
       in a single resize step (no flicker, no intermediate sizes).
     - Watch the dev log: one `apply_board_size` invocation per tier
       transition; sitting on the same station for 60 s (two poll
       ticks) MUST NOT issue any extra resize requests. The renderer-
       side dedupe (`lastSizeKey`) is what protects the main-thread
       Cocoa dispatch.

## Branching rules (also in `~/.claude/CLAUDE.md`)

- Never commit to `main` directly. Always feature-branch.
- Never `--no-verify`. If a hook fails, fix the underlying issue.
- Squash-merge with `--delete-branch`. No rebase-merge, no merge-commit.
- The user runs the merge themselves unless explicitly asked.

## External consumers of `crates/tfl-*`

The three core crates (`tfl-domain`, `tfl-client`, `tfl-board`) are consumed
by [`argen/tubbie-ios`](https://github.com/argen/tubbie-ios) via a SHA-pinned
git submodule. Their public surface is now a contract — see
`docs/ADR/crates-as-public-contract.md`. Breaking changes to public symbols
require coordination (paired PR + submodule bump in tubbie-ios). Internal
refactors are unaffected.

## Things that look correct in isolation but break the integration

- A passing `BoardService::stream` test does NOT prove `save_config`
  reaches the stream — those tests use a `watch::channel` directly,
  not `AppState.cfg_tx`. You need a `fixture_state_with_stream` test.
- `fixture_state()` quietly accepts `cfg_tx.send` calls because of
  `let _ = ...`, so end-to-end command tests using it can pass while
  the pipeline is fundamentally broken. Never use it for `save_config`
  scenarios.
- A clean `cargo test` does NOT mean the user-visible UX is correct.
  You must run the manual smoke above for any stream/config change.
- "I added a test" is meaningless if the test uses the wrong fixture.
  Verify the test would actually fail without your fix by reverting the
  fix locally and watching the test go red.

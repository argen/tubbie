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

3. **Filter / theme / `line_ids` / `directions` changes MUST NOT
   trigger a fresh fetch.** That was the whole point of the watch-channel
   refactor: chip-toggle bursts must coalesce. The
   `save_config_filter_change_does_not_force_immediate_refresh` test
   asserts the no-fetch behaviour.

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

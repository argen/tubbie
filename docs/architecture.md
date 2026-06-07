# Architecture

How the pieces of tubbie fit together. For *why* individual decisions were made, see [`ADR/`](ADR/README.md). For build and dev workflow, see the top-level [`README.md`](../README.md).

## Crate graph

```
                  ┌──────────────┐
                  │  tfl-domain  │  pure types + formatters, zero I/O
                  └──────┬───────┘
                         │
               ┌─────────┴──────────┐
               ▼                    ▼
       ┌──────────────┐      ┌────────────────┐
       │  tfl-client  │      │   tfl-board    │
       │  TflClient   │◀─────│  BoardService  │
       │  TflHttp     │      │  streaming,    │
       │  ReqwestTflHttp      │  filtering,   │
       │  FixtureTflHttp      │  stale-data   │
       │  Clock                └───────┬────────┘
       └──────┬───────┘                │
              │                        │
              │    ┌──────────────┐    │
              │    │ fixture-     │    │
              │    │ recorder bin │    │
              │    └──────────────┘    │
              │                        │
              ▼                        ▼
        fixtures/*.json        src-tauri (Tauri shell)
                                       │
                                       ▼
                                 web/ (SvelteKit)
```

- **`tfl-domain`** — `Arrival`, `Station`, `Line`, `Direction`, `LineStatus`, `Board`, `Platform`, `Theme`, plus formatters (`format_time_to_station`, `group_by_platform`, Northern-line branch inference) and the `is_supported_line_id` whitelist (Tube + DLR + Overground six named lines + legacy `london-overground` + Elizabeth). Depends on `serde` + `chrono` only.
- **`tfl-client`** — the `TflHttp` trait (transport seam), its two impls (`ReqwestTflHttp` live, `FixtureTflHttp` offline), the `Clock` trait + impls, `TflError`, and `TflClient<H>` with `get_arrivals` / `search_stations` / `get_line_status` / `allowed_line_ids_for`. Surfaces a configurable mode set (`SUPPORTED_MODES = [tube, overground, dlr, elizabeth-line]`); downstream consumers like the iOS shell can opt down via `TflClient::with_modes(http, &["tube"])`. Owns the multi-mode caches: `stop_points_cache` (one merged `Vec<Station>` deduped by id with hub-merged lines, refreshed via a `tokio::sync::Mutex` single-flight gate), `hub_children_cache` and `hub_lines_cache` (per-process, populated lazily from `/StopPoint/{HUB_ID}` with NotFound results cached as empty), `line_status_cache` (60 s TTL, fan-out across all surfaced modes).
- **`tfl-board`** — `BoardService<H, C>` with one-shot `refresh()` and a polling `stream()` (missed-tick-skip, stale-data fallback, deterministic termination after fatal error). Applies user filters (`line_ids`, `directions`) and the defensive `drop_arrivals_for_lines_not_serving` post-filter (uses `allowed_line_ids_for` as the source of truth). Owns no state beyond what lives inside the stream's unfold closure.
- **`fixture-recorder`** — dev-only binary. Hits the live TfL API anonymously, sanitises `app_key` out of URLs, writes atomically via `.tmp` + rename, trims the bloated stop-points response to the fields we actually use (preserving `hubNaptanCode` so hub-merge tests work). Captures all surfaced modes plus the hub stop-points the multi-mode tests exercise.
- **`src-tauri`** — Tauri v2 shell. Constructs `AppState { board_service, config_store, cfg_tx, … }` at startup, registers commands, spawns the polling stream and re-emits boards as `board://updated` events. On a fresh stream-error streak with no last-ok board, it also emits `board://error` (`{ message: string }`) once per streak so the renderer can surface "we have nothing to show" without staring at a forever-loading state. `load_config_inner` runs `migrate_legacy_line_ids` on every load so a stored `BoardConfig.line_ids` containing the legacy `"london-overground"` id is rewritten to the six named successors before the watch-channel publishes it. `pool_key.rs` handles zero-config onboarding: at startup it reads a cached pool key from the config store (no network on the startup path) and bakes it into the one shared `Arc<TflClient>`; a background task refreshes the cache from `tubbie.brunobelcastro.com/pool-keys.json` for the next launch. Every failure mode returns `None` (fail-open to anonymous). `apply_tray_disruption` swaps the menu-bar icon between the normal dot-matrix glyph and an alert variant when a watched line is disrupted.
- **`web/`** — SvelteKit single-page app bundled by `adapter-static`. Subscribes to `board://updated` + `board://error`, seeds via a one-shot `get_board` on startup, debounces `search_stations` with cancel-in-flight, switches themes by toggling CSS custom properties. The Settings chip list (`KNOWN_LINES` in `routes/settings/+page.svelte`) carries 19 entries today — 12 Tube + DLR + 6 named Overground lines + Elizabeth — pruned to the picked station's `lines` field at runtime. `<StatusPanel>` renders a worst-first marquee ticker of disrupted lines (embedded in the board footer); toggling the status button in the board header replaces the arrivals area with `<StatusView>`, which covers all TfL lines with affected route segments, expandable disruption prose, and a "Good service on all other lines" footer. `<StationSearch>` is embedded in the board header so users can change station without opening Settings.

## Data flow (steady state)

1. User launches app. `src-tauri/src/lib.rs::run()` loads `BoardConfig` from `tauri-plugin-store`, constructs `ReqwestTflHttp` (anonymous or keyed from the store), and spawns a task that polls `BoardService::stream(config)`.
2. Each emitted `Board` is serialised and `emit("board://updated", &board)` is fired to the main window.
3. The frontend's `+layout.svelte` registered a `listen("board://updated")` before the seed fetch; events land in `$board` (Svelte 5 `$state`).
4. `<Board>` renders platforms as columns or rows; `<ArrivalRow>` is keyed on `(line_id, platform_name, expected_arrival)` so the same logical train is stable across polls. The composite key is a deliberate choice: TfL's `Arrival.id` is **not** a unique identifier (observed at Chalk Farm: 10 distinct trains all returned with `id=1731547612`), so a naive `(arrival.id)` key would crash Svelte's keyed-each with `each_key_duplicate` whenever a station's predictions collide. `PlatformColumn.svelte` also dedupes defensively at the keyed-each boundary by the same composite, so any future surprise in the data shape can't re-introduce that crash.
5. `<StatusPanel>` renders a worst-first marquee of disrupted lines from the same `Board` payload (line-status is embedded there). Toggling the status button swaps the arrivals area for `<StatusView>`, a full network-wide status breakdown fetched via `get_all_line_statuses` (a separate Tauri command backed by the shared `line_status_cache`). Disruption text is always plain-text — no `{@html}` anywhere.
6. On `prefers-reduced-motion: reduce`, a shared `reducedMotion` store short-circuits each animation to a static state change — enforced by DOM tests.

## Config change flow

1. User saves config in Settings.
2. `save_config_inner` validates, persists via `tauri-plugin-store` (blocking I/O wrapped in `spawn_blocking`), and publishes the new `BoardConfig` to the running stream task via `state.cfg_tx.send(cfg)` — the watch channel.
3. The stream observes the change in its `tokio::select!` via `cfg_rx.changed()`. Station-id changes trigger an immediate refresh (CLAUDE.md invariant #2); filter-only changes (line_ids / directions / theme / poll_seconds) wait for the next tick (invariant #3) so chip-toggle bursts coalesce without burning the rate-limit budget.
4. Frontend seed-fetches `get_board` once on layout mount to avoid a render gap while the stream is warming up; `applyBoard`'s "latest wins by `generated_at`" check resolves races.

## Multi-mode topology (stop-points + line-status caches)

The client surfaces four TfL modes — Tube, Overground, DLR, Elizabeth — to keep the search dropdown and the chip filter aligned with what the live arrivals endpoint will actually return.

- **`stop_points_cached`** fans out one `/StopPoint/Mode/{mode}` fetch per entry in `tfl_client::SUPPORTED_MODES` via `futures::future::join_all`, parses each into `Vec<Station>`, and merges them keyed by `Station.id` with a line-list union. Stations carrying a `hub_naptan_code` then get a second-stage hub-merge that walks `/StopPoint/{HUB_ID}` for sibling stop-points and unions in their lines (so Bank's tube parent advertises DLR; Whitechapel advertises Elizabeth + Windrush). The hub jobs are deduped by `hub_naptan_code` before fan-out — without it, a hub like `HUBKGX` referenced by ~23 stations would fire 23 redundant TfL requests in parallel (CLAUDE.md invariant #17).
- **Single-flight refresh** — concurrent refreshes serialise behind a `tokio::sync::Mutex` so the periodic background tick and a cold-cache search call can't both fan out at once (invariant #16).
- **Stale-while-revalidate** — `stop_points_cached` returns whatever's currently cached (fresh or stale) and only blocks on the network for a truly cold cache. A periodic task spawned in `lib.rs::run` calls `refresh_stop_points_cache` every ~14 minutes (just under `STOP_POINTS_TTL`) to keep the cache fresh out-of-band. User-facing `search_stations` and the hub-merge lookups in `get_arrivals` therefore never block on a TTL-driven refresh past the initial warm; if the periodic tick is missed (laptop sleep, transient TfL outage) the user sees slightly older station metadata until the next tick — acceptable because TfL station metadata is stable for months (invariants #19 + #20).
- **Stale-but-usable** — `read_fresh_cache` gates whether `refresh_stop_points_inner` should short-circuit on cold-cache callers, but `resolve_arrival_ids` and `allowed_line_ids_for` use `read_cache_any`, which returns whatever's cached regardless of TTL. This means a 15-min TTL boundary doesn't break hub-merge for arrivals at hub stations even if the periodic refresh hasn't fired yet (invariant #19).
- **`get_line_status`** runs a parallel `line-status/{mode}` fan-out into a single `Vec<TflLine>` cache stamped with one `Instant`; the per-line lookup is unchanged.
- **Search dedupe** — at multi-mode interchanges (Bank, Farringdon) the per-mode feeds each return their own canonical entry sharing a `hub_naptan_code`. `search_stations` keeps one canonical row per hub code with the priority `940GZZLU` (Tube) > `940GZZDL` (DLR) > `910G` (Overground / Elizabeth) so the dropdown isn't full of near-duplicates that route to the same arrivals via hub-merge (invariant #18).
- **Legacy id migration** — `load_config_inner` rewrites any stored `"london-overground"` line id into the six successor ids on load, in stable order, deduped against existing entries (invariant #14). Idempotent. Keeps users upgrading from a pre-November-2024 install from silently losing their Overground board.

## Key abstractions and trait seams

| Seam | Purpose | Fake |
|---|---|---|
| `TflHttp` | Transport boundary — `fetch(endpoint, id) -> Result<Value, TflError>` | `FixtureTflHttp` reads `fixtures/{endpoint}/{id}.json` |
| `Clock` | Time source for `generated_at` / `stale_since` / `FakeClock`-driven tests | `FakeClock::at(...)` |
| `ConfigStore` | Atomic `load_config` / `save_config` / `load_app_key` / `save_app_key` | `MemoryConfigStore` for headless tests |
| `AnyBoardService` | Object-safe erasure for `BoardService<H, C>` so `AppState` holds `Arc<dyn …>` | Tauri tests inject a fixture-backed instance |

## Error model

- `TflError::{NotFound, InvalidRequest, Parse, ParseAt, Transport, RateLimited, Http}` — consumed by `TflClient` callers. `Transport`'s `Display` has the query string stripped; URL never leaks `app_key`.
- `BoardError` wraps `TflError` where board-layer callers need a distinct type.
- Tauri commands convert errors to `Result<T, String>` at the IPC boundary, using `Display` (already redacted).
- Stream errors fall through as stale-state emissions when a prior board exists. When there is no last-ok board the stream's `Err(BoardError)` is logged once per streak by `spawn_stream_task` and emitted to the renderer as a `board://error` event so the user can see _something_ instead of a forever-loading state. Recovery is implicit: the next successful tick emits `board://updated`, which the frontend's `applyBoard` already uses to clear `boardError`.
- The stop-points cache warm task at startup awaits the first `board://updated` (or an 8 s fallback) before firing. The `/StopPoint/Mode/{mode}` endpoints are TfL's most aggressively rate-limited routes; firing the four-mode fan-out concurrently with the stream's first `/Arrivals` fetch could 429 and set the shared `cooldown_until` gate, blocking the stream's first emit for nothing — the user is staring at the board on launch, not at settings. Once the warm completes, the merged cache also resolves the per-station hub-merge that the chip UI and the defensive arrivals filter both rely on.

## Security surfaces

| Surface | Control |
|---|---|
| TfL `app_key` | `zeroize`-on-drop `AppKey`, stripped from error `Display`, never in fixtures, never pre-populated into the Settings input field |
| `FixtureTflHttp::fetch` path | `endpoint`/`id` must match `[a-zA-Z0-9_-]{1,64}`; rejected before any filesystem call |
| Tauri command inputs | `station_id` / `line_id` / `query` / `app_key` / `BoardConfig` validated explicitly; `line_ids.len() ≤ 32`, `directions.len() ≤ 16`, `poll_seconds` clamped `[5, 300]` |
| CSP | `default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self' https://api.tfl.gov.uk; img-src 'self' data:; font-src 'self'` — no `unsafe-inline` |
| Capabilities | `core:default` + window close/minimize/maximize/set-title + `store:default`. No `shell`, `fs`, `dialog`, `http`, `path`. |
| TfL disruption text | Plain-text interpolation only; no `{@html}` anywhere in `web/src/` |

## Testing strategy

- **Rust default gate** (`just verify-rust`): `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test --workspace`. Zero network, deterministic via `FixtureTflHttp` + `FakeClock`.
- **Rust live gate** (`just verify-live`): developer-triggered only. Runs `--features live` integration tests against `api.tfl.gov.uk`. Not wired into CI.
- **Web gate** (`npm run verify` inside `web/`): ESLint v10 flat config (type-checked strict), Prettier, `svelte-check`, Vitest (`node` env default; `// @vitest-environment happy-dom` per-file for DOM tests).
- **Snapshot**: `crates/tfl-domain/tests/board_format.rs` renders a Belsize Park fixture through the formatter and `insta::assert_snapshot!`s the result — a canary for any rendering regression.
- **Render-count test**: `web/src/lib/__tests__/dom/board-rerender-count.dom.test.ts` pins the no-full-remount guarantee under stream emissions.

## Fixture catalogue

| Path | Purpose | Size |
|---|---|---|
| `fixtures/arrivals/940GZZLU{BZP,KSX,BNK,OXC,HAI}.json` | Tube arrivals per station (Belsize Park / King's Cross / Bank / Oxford Circus / Highbury & Islington) | 11–83 KB |
| `fixtures/arrivals/910GHACKNYC.json` | Overground-only arrivals (Hackney Central, Mildmay) — covers the OG-only test path end-to-end | ~40 KB |
| `fixtures/line-status/{tube,overground,dlr,elizabeth-line}.json` | Per-mode line-status payloads for the merged `Vec<TflLine>` cache | 1–15 KB each |
| `fixtures/stop-points/{tube,overground,dlr,elizabeth-line}.json` | Trimmed per-mode stop-point lists merged by `stop_points_cached` for `search_stations` and hub lookups | 38–454 KB each |
| `fixtures/stop-point/HUB{BAN,TCR,ZWL,HHY}.json` | Hub stop-point detail for the multi-mode interchanges the search-merge tests exercise (Bank / TCR / Whitechapel-renamed-from-HUBWHC / Highbury & Islington) | 93–319 KB each |
| `crates/tfl-board/tests/data/phantom_overground_at_belsize_park.json` | Hand-crafted phantom Mildmay arrival at a Tube-only station — drives the defensive-filter regression test for Overground line ids | small |
| `fixtures/**/*.meta.json` | Sidecar recording time + sanitised URL | small |

Refresh via `just record-fixtures` (hits the live TfL API; the trim step is baked into the recorder so the stop-points fixtures never regress to their multi-MB raw form, and `hubNaptanCode` is preserved through trim because production hub-merge needs it).

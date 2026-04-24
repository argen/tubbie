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

- **`tfl-domain`** — `Arrival`, `Station`, `Line`, `Direction`, `LineStatus`, `Board`, `Platform`, `Theme`, plus formatters (`format_time_to_station`, `group_by_platform`, Northern-line branch inference). Depends on `serde` + `chrono` only.
- **`tfl-client`** — the `TflHttp` trait (transport seam), its two impls (`ReqwestTflHttp` live, `FixtureTflHttp` offline), the `Clock` trait + impls, `TflError`, and `TflClient<H>` with `get_arrivals` / `search_stations` / `get_line_status`.
- **`tfl-board`** — `BoardService<H, C>` with one-shot `refresh()` and a polling `stream()` (missed-tick-skip, stale-data fallback, deterministic termination after fatal error). Owns no state beyond what lives inside the stream's unfold closure.
- **`fixture-recorder`** — dev-only binary. Hits the live TfL API anonymously, sanitises `app_key` out of URLs, writes atomically via `.tmp` + rename, trims the bloated stop-points response to the fields we actually use.
- **`src-tauri`** — Tauri v2 shell. Constructs `AppState { board_service, config_store }` at startup, registers commands, spawns the polling stream and re-emits boards as `board://updated` events, aborts the stream cleanly on window close or config change.
- **`web/`** — SvelteKit single-page app bundled by `adapter-static`. Subscribes to `board://updated`, seeds via a one-shot `get_board` on startup, debounces `search_stations` with cancel-in-flight, switches themes by toggling CSS custom properties.

## Data flow (steady state)

1. User launches app. `src-tauri/src/lib.rs::run()` loads `BoardConfig` from `tauri-plugin-store`, constructs `ReqwestTflHttp` (anonymous or keyed from the store), and spawns a task that polls `BoardService::stream(config)`.
2. Each emitted `Board` is serialised and `emit("board://updated", &board)` is fired to the main window.
3. The frontend's `+layout.svelte` registered a `listen("board://updated")` before the seed fetch; events land in `$board` (Svelte 5 `$state`).
4. `<Board>` renders platforms as columns or rows; `<ArrivalRow>` is keyed on arrival id so only changed rows re-render.
5. `<LineStatusTicker>` subscribes to the same emissions (line-status is embedded in the `Board` payload); disruption text is rendered as plain text and CSS-marquee-animated.
6. On `prefers-reduced-motion: reduce`, a shared `reducedMotion` store short-circuits each animation to a static state change — enforced by DOM tests.

## Config change flow

1. User saves config in Settings.
2. `save_config` command validates, persists via `tauri-plugin-store` (blocking I/O wrapped in `spawn_blocking`), aborts the current stream task, spawns a replacement with the new config.
3. Frontend seed-fetches `get_board` to avoid a render gap while the new stream is warming up; timestamps resolve races.

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
- Stream errors fall through as stale-state emissions when a prior board exists; they only surface as `Err(BoardError)` if there's nothing to fall back on, and the stream terminates cleanly after that.

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
| `fixtures/arrivals/{940GZZLUBZP,940GZZLUKSX,940GZZLUBNK,940GZZLUOXC}.json` | Arrivals per station (BZP / KSX / BNK / OXC) | 7–24 KB |
| `fixtures/line-status/tube.json` | All tube-line statuses | 33 KB |
| `fixtures/stop-points/tube.json` | Trimmed tube stop-point list for `search_stations` | 430 KB |
| `fixtures/**/*.meta.json` | Sidecar recording time + sanitised URL | small |

Refresh via `just record-fixtures` (hits the live TfL API; the trim step is baked into the recorder so the stop-points fixture never regresses to the 23 MB raw form).

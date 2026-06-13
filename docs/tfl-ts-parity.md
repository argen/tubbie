# TfL Rust → TypeScript parity checklist

Living map from each numbered invariant in [`CLAUDE.md`](../CLAUDE.md) to its
TypeScript test in the ported core under `web/src/lib/tfl/`. The port (Phases
0–5 of the source plan) reproduces the Rust crates' behaviour in the webview
behind the `USE_TS_TFL` flag; this table is the proof that every guarded
contract crossed the language boundary with a test, not just code.

**Three categories:**

- **Ported** — the logic moved to TS and carries a named TS test.
- **Stays Rust** — the invariant lives in the Tauri shell (native chrome,
  persistence, config seed) and is intentionally _not_ ported; the TS path
  reuses it over IPC. Its Rust test remains the guard.
- **Frontend** — already a webview concern (Svelte rendering); its existing DOM
  test covers both data paths unchanged.

| #   | Invariant (short)                                  | Category   | TS test (or note)                                                                                                                      |
| --- | -------------------------------------------------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `save_config` publishes to a live watch receiver   | Stays Rust | Config persistence stays IPC. TS analogue: `setStreamConfig` push — `__tests__/dom/tfl-stream-wiring.dom.test.ts` (config→stream seam) |
| 2   | station_id change → immediate refresh              | Ported     | `board/stream.test.ts` "refreshes immediately on a station change (#2)"; wiring: `tfl-stream-wiring` setStreamConfig                   |
| 3   | filter / theme / directions → no fresh fetch       | Ported     | `board/stream.test.ts` "does not refetch on a filter change (#3)"                                                                      |
| 4   | stream is infinite; never `break` on `Err`         | Ported     | `board/stream.test.ts` stale re-emit (#4) + error-without-last-ok (#4)                                                                 |
| 5   | one shared `Arc<TflClient>` process-wide           | Ported     | `tfl/runtime.ts` memoized singleton (composition root); shared client exercised by `cache/tflClient.test.ts` (#5) + the routing test   |
| 6   | initial config loads synchronously before stream   | Stays Rust | Config seed via IPC; `+layout.svelte` awaits `initConfig()` before `startBoardSubscription($config)`                                   |
| 7   | `generated_at` latest-wins in the frontend         | Frontend   | `__tests__/dom/board-store-seed.dom.test.ts` (existing `applyBoard`; reused unchanged by the TS path)                                  |
| 8   | display-mode side-effects on the macOS main thread | Stays Rust | Native shell. (`board/stream.test.ts` tags #8 for lifecycle pause/resume, but the Cocoa barrier is Rust)                               |
| 9   | `apply_board_size` hops to the main thread         | Stays Rust | Native shell (`NSWindow::setFrame:`)                                                                                                   |
| 10  | drop arrivals for lines not serving (family key)   | Ported     | `board/filters.test.ts` "keeps a sibling Overground line via the family key (#10)" + `domain/lines.test.ts` `lineFamilyKey`            |
| 11  | line-grouped UI re-buckets per-line                | Frontend   | `Board.svelte` `linesGrouped`; existing `__tests__/dom/board-line-groups.dom.test.ts`                                                  |
| 12  | caches fetch per-mode and merge                    | Ported     | `cache/tflClient.test.ts` (#12) + `cache/hubCompleteness.test.ts`                                                                      |
| 13  | search uses a NaPTAN canonical-prefix whitelist    | Ported     | `cache/tflClient.test.ts` (#13)                                                                                                        |
| 14  | legacy `london-overground` ids migrate on load     | Stays Rust | `migrate_legacy_line_ids` in `commands.rs` (config stays IPC; single migration site)                                                   |
| 15  | hub-line cache caches `NotFound`                   | Ported     | `cache/tflClient.test.ts` (#15)                                                                                                        |
| 16  | `stop_points_cached` single-flights refreshes      | Ported     | `cache/tflClient.test.ts` (#16) — single-flight via a stored Promise                                                                   |
| 17  | hub fan-out dedupes by `hub_naptan_code`           | Ported     | `cache/tflClient.test.ts` (#17)                                                                                                        |
| 18  | `search_stations` dedupes canonical hub entries    | Ported     | `cache/tflClient.test.ts` (#18)                                                                                                        |
| 19  | `allowed_line_ids_for` reads `read_cache_any`      | Ported     | `cache/tflClient.test.ts` "serves a stale cache past the TTL (#19)"                                                                    |
| 20  | stop-points cache is SWR; refresh out-of-band      | Ported     | `cache/tflClient.test.ts` (#20) + `tfl/runtime.ts` ~14-min periodic refresh                                                            |
| 21  | per-mode stop-points fetch retries on transient    | Ported     | `cache/tflClient.test.ts` (#21)                                                                                                        |
| 22  | `line_ids` chip filter is a frontend-only mask     | Ported     | `board/filters.test.ts` "treats line_ids as a no-op (#22)"                                                                             |
| 23  | Elizabeth / named OG surface as compass directions | Ported     | `domain/direction.test.ts` (#23, compass-from-towards)                                                                                 |
| 24  | drop arrivals terminating at the queried station   | Ported     | `board/filters.test.ts` "drops an arrival whose destination is the queried station (#24)"                                              |
| 25  | `severity_bucket` is the single canonical mapping  | Ported     | `domain/status.test.ts` `severityBucket` (#25)                                                                                         |
| 26  | partial-warm uses a short retry window             | Ported     | `cache/tflClient.test.ts` (#26)                                                                                                        |

## Wiring (Phase 5) — no invariant number, but contract-bearing

| Contract                                                   | TS test                                                                           |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Flag off → Rust path untouched (read commands)             | `__tests__/dom/tfl-flag-routing.dom.test.ts` (flag-OFF cases)                     |
| Flag off → board still seeds via `get_board`, no TS stream | `__tests__/dom/tfl-stream-wiring.dom.test.ts` "Rust path (flag OFF) is untouched" |
| Flag on → read commands route to the TS client             | `tfl-flag-routing` (flag-ON cases)                                                |
| Flag on → board driven by `BoardStream`, first emit = seed | `tfl-stream-wiring` "emits an immediate board from the stream"                    |
| `updateConfig` → stream after a successful save (#2)       | `tfl-stream-wiring` "config.updateConfig → stream"                                |

## Known deferred parity gap

**Staleness re-emit (both paths).** On a fetch failure the stream re-emits the
last-ok board with `stale_since` set but the **same** `generated_at` (Rust
`service.rs` and the TS `BoardStream` both behave this way). `applyBoard`'s
`generated_at >= current` guard (invariant #7) therefore drops it, so the STALE
badge never lights via a re-emit. This is identical on both paths, so the port
preserves parity; surfacing staleness needs a `stale_since`-aware override in
`applyBoard` applied to **both** paths — a separate change, deliberately not made
during the behaviour-neutral wiring phase. See the comment at the guard in
`web/src/lib/stores/board.ts`.

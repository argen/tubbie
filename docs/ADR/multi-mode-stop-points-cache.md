# ADR: Multi-mode stop-points cache (Tube + Overground + DLR + Elizabeth)

**Status:** Accepted

## Context

In November 2024 TfL split the London Overground into six independently-named lines (Mildmay, Lioness, Suffragette, Windrush, Weaver, Liberty), each with its own line id and brand colour. Tubbie originally surfaced Tube only — `stop_points_cached` fetched `/StopPoint/Mode/tube`, `get_line_status` fetched `/Line/Mode/tube/Status`, and `search_stations` filtered to `id.starts_with("940GZZLU")`. That left:

- Overground-only stations (Hackney Central, Dalston Junction, …) unsearchable
- DLR-only stations (Mudchute, Beckton, …) silently filtered out by the prefix gate (a latent bug that pre-dated the rollout)
- Elizabeth-line-only stops (Custom House, Abbey Wood) similarly hidden
- Mixed-mode hubs (Bank, Whitechapel, Highbury & Islington) showing only their Tube lines in the chip filter, with the defensive `drop_arrivals_for_lines_not_serving` filter dropping every legitimate non-Tube arrival because the per-station "allowed line set" was Tube-only

The rollout extends `tfl-client` to surface four modes in parallel — Tube, Overground, DLR, Elizabeth — without breaking the rate-limit budget, the offline-fixture testing model, or downstream consumers (the iOS shell submodules `crates/tfl-*`).

## Decisions

### 1. Per-mode fan-out, single merged cache

`stop_points_cached` runs one `/StopPoint/Mode/{mode}` fetch per entry in `tfl_client::SUPPORTED_MODES = ["tube", "overground", "dlr", "elizabeth-line"]` via `futures::future::join_all`, then merges the results into a single `Vec<Station>` keyed by `Station.id` (line-list union on collision; `hub_naptan_code` backfilled from whichever feed has it).

Same pattern for `get_line_status`: a parallel `line-status/{mode}` fan-out concatenates into one `Vec<TflLine>` cache stamped with one `Instant`. Per-line lookup is unchanged.

**Why not a comma-separated multi-mode endpoint string?** The `FixtureTflHttp::fetch` validator enforces `[a-zA-Z0-9_-]{1,64}` on `endpoint` and `id` to prevent path traversal in offline tests. A `tube,overground,dlr,elizabeth-line` id violates that, and widening the regex would weaken a deliberate security mitigation. Per-mode fixtures are also easier to record, diff, and reason about.

### 2. Configurable mode set via `TflClient::with_modes`

`TflClient::new(http)` defaults to `SUPPORTED_MODES`. `TflClient::with_modes(http, modes: &'static [&'static str])` lets a downstream consumer (notably the iOS shell, where memory and bandwidth budgets are tighter) opt down to a subset, e.g. `&["tube", "overground"]`. Modes outside `SUPPORTED_MODES` are accepted by the constructor and produce `NotFound` from the fixture transport when their JSON is absent — an extension point for a consumer that wants to ship its own mode without us having to teach the constructor about it.

### 3. Hub-merge by `hub_naptan_code`, deduped before fan-out

`Station.lines` for a hub-bearing station gets enriched with the union of its hub siblings' lines so the chip UI shows DLR / Elizabeth / Overground chips at Bank / Whitechapel / Stratford. The naive approach iterates `stations.iter().enumerate()` and fires one `hub_lines_cached(hub_id)` job per station, but a single hub like `HUBKGX` is referenced by ~23 stations across the four mode feeds — that's 757 simultaneous TfL requests for 90 unique hubs (8.4× redundancy). All 23 racers for a given hub see an empty `hub_lines_cache` and each fires its own HTTP request before any of them can populate it.

We dedupe by building `stations_per_hub: HashMap<hub_id, Vec<usize>>` before the fan-out so each unique hub is fetched once and the result is merged into every station that references it. Guarded by `warm_stop_points_dedupes_hub_fetches_before_fan_out`.

### 4. Single-flight refresh

The sync `Mutex<Option<StopPointsCacheEntry>>` cache check is fast, but releasing the lock and starting an async fan-out lets a debounced search burst (200 ms typing → three keystrokes within a TTL-expiry window) each see an empty cache and fire its own full per-mode + hub fan-out — three parallel `~3-second` warms against TfL.

We add a `tokio::sync::Mutex<()>` (`stop_points_refresh`) that serialises refreshes: the first caller does the work; subsequent callers `await` the lock, then re-check the cache and return immediately. The lock is held across the per-mode + hub fan-out so the second caller's re-check sees the freshly-stamped cache. Read-only callers (post-warm cache hits) never touch this lock.

### 5. Stale-but-usable lookups (`read_cache_any` vs `read_fresh_cache`)

The 15-min `STOP_POINTS_TTL` decides when `refresh_stop_points_inner` should short-circuit (a concurrent caller may have already produced fresh data while we were waiting on the lock), not when the cache stops being *useful*. Past the TTL boundary, a cached entry's `hub_naptan_code` and `lines` fields remain valid — TfL station metadata changes infrequently (months, not minutes).

We split the cache reader into two:

| Reader | Returns | Used by |
|---|---|---|
| `read_fresh_cache` | `Some(Vec<Station>)` only if entry age `< STOP_POINTS_TTL` | `refresh_stop_points_inner`, to short-circuit non-forced cold-cache callers |
| `read_cache_any` | `Some(Vec<Station>)` whenever any cached entry exists | `stop_points_cached`, `resolve_arrival_ids`, `allowed_line_ids_for` |

Without `read_cache_any`, the first stream tick after expiry loses hub-merge for arrivals (Bank/Euston/Whitechapel siblings are never fetched) and the user's chip filter silently sees zero arrivals at hub stations because the only data path that could return Overground/DLR/Elizabeth predictions (the OG sibling fetch) was bypassed. Guarded by `allowed_line_ids_for_serves_stale_cache_past_ttl`.

### 5b. Stale-while-revalidate: refresh runs out-of-band

`stop_points_cached` returns whatever's currently cached (fresh or stale, via `read_cache_any`) and only blocks on the network for a truly cold cache. A periodic task spawned in `lib.rs::run` calls the public `refresh_stop_points_cache` every ~14 minutes (just under `STOP_POINTS_TTL`) to keep the cache fresh out-of-band. The public refresh runs the single-flighted fan-out + hub-merge unconditionally (`refresh_stop_points_inner(force = true)`); the cold-cache path uses `force = false` so a concurrent refresher's stamp short-circuits us.

User-facing `search_stations` and the hub-merge lookups in `get_arrivals` therefore never block on a TTL-driven refresh past the initial warm. If the periodic tick is missed (laptop sleep, transient TfL outage) the user sees slightly older station metadata until the next tick — acceptable because TfL station metadata is stable for months. Guarded by `search_stations_does_not_refetch_when_cache_is_stale_but_present` and `refresh_stop_points_cache_forces_refetch_even_when_fresh`.

### 6. Search dedupe by `hub_naptan_code`

At multi-mode interchanges (Bank, Farringdon, …) the per-mode `/StopPoint/Mode/{mode}` feeds each return their own canonical entry — `940GZZLUBNK` and `940GZZDLBNK` both have `hubNaptanCode: HUBBAN` because they're the same physical station. After hub-merge they also carry the same union of lines, so the dropdown would show two near-identical rows that route to the same arrivals via the hub-merge fan-out in `get_arrivals`.

`search_stations` keeps one canonical entry per `hub_naptan_code` with the prefix priority `940GZZLU` (Tube) > `940GZZDL` (DLR) > `910G` (Overground / Elizabeth). The user's mental model maps a hub to its Tube parent at every interchange we surface today. Stations whose `hub_naptan_code` is `None` (Hampstead Heath, Belsize Park, single-mode stops) pass through unchanged.

### 7. Hub-line cache: cache `NotFound` results

`hub_lines_cached` stores an empty `Vec<LineRef>` on `TflError::NotFound` (a hub the live API genuinely doesn't expose) so the next cold-warm doesn't re-fire 187 known-404 requests. Transient errors (transport, rate-limited) are NOT cached — a 429 must retry on the next warm. Without this distinction, a single warm cycle fans out 190+ hub fetches every TTL boundary; with it, the count is bounded by hubs whose data has ever resolved.

### 8. Legacy `"london-overground"` config migration

A user upgrading from a pre-November-2024 install may have a stored `BoardConfig.line_ids` containing the legacy `"london-overground"` id. The live API no longer emits predictions under that id. `load_config_inner` calls `migrate_legacy_line_ids` to expand any `"london-overground"` entry into the six successor ids `[mildmay, lioness, suffragette, windrush, weaver, liberty]` at the same position, deduping against existing entries. Idempotent. The legacy id stays in `is_supported_line_id` so historical fixtures still parse.

### 9. Search prefix whitelist (with mode-filter for `910G`)

Replaced the original `id.starts_with("940GZZLU")` filter with a NaPTAN-prefix whitelist:

- `940GZZLU*` — London Underground canonical
- `940GZZDL*` — DLR canonical (closes a latent bug where DLR-only stations were silently excluded)
- `910G*` filtered to stops whose `modes` list includes `overground` or `elizabeth-line` — the 910G NaPTAN range overlaps with National Rail-only operators (Gatwick Express, Thameslink, Southern, …) which we don't surface

Excluded by absence: `9400ZZLU*`, `4900*`, `2100*` (platform-level children that would duplicate rows) and `HUB*` (multi-mode aggregators with no stable arrivals id).

## Consequences

**Positive:**
- All four modes searchable, filterable, and rendered with correct branded colours (the CSS variables, label maps, and `is_supported_line_id` whitelist were all already in place from a prior staging branch).
- Multi-mode hubs (Bank / Whitechapel / Farringdon / Stratford) advertise the full chip set and route arrivals through the existing hub-merge fan-out untouched.
- Search dedupe removes duplicate-feeling rows at interchanges.
- Single-flight + hub-dedupe keep the warm cost bounded and serialised even under a debounced-keystroke burst.
- Stale-but-usable lookups keep the chip filter working across TTL boundaries.

**Negative / accepted trade-offs:**
- The merged `stop_points_cache` grows from ~16 MB (Tube only) to ~22 MB (all four modes). Acceptable on desktop; flagged for the iOS shell, which can opt down via `with_modes`.
- The cold-warm wall-clock time is dominated by TfL response latency for the four parallel mode fetches plus the 90-hub burst — typically a few hundred ms with an `app_key` (500 req/min budget) but can reach ~1 s on a slow network. Single-flight + hub-dedupe prevent this from compounding under concurrent callers.
- `search_stations` no longer returns the secondary canonical entry at a hub — a user who specifically wants "Bank DLR Station" as a label sees "Bank Underground Station" instead. They route to the same arrivals data via hub-merge so functionally equivalent; UI label is the only delta.
- `load_config_inner` runs migration on every load (microseconds; idempotent). The initial cfg loaded by `lib.rs::run` for the watch-channel still goes through `config_store.load_config()` directly without migration — a known asymmetry that doesn't affect behaviour today (the frontend's `loadConfig` IPC always re-loads through the migration on app start before save events) but should be tightened in a follow-up.

**Operational:**
- New `cargo test` and `npm test` regression rows added under CLAUDE.md invariants 12–19.
- The fixture-recorder now captures all four modes plus `HUB{BAN,TCR,ZWL,HHY}` hub stop-points; running `just record-fixtures` refreshes the full set.
- TfL renamed the Whitechapel hub from `HUBWHC` to `HUBZWL` mid-rollout. The recorder caught it; future hub renames will surface the same way (a re-record produces a missing hub fixture warning).

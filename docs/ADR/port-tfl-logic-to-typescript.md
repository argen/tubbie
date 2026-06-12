# ADR: Port TfL logic from Rust into the TypeScript frontend

**Status:** Accepted (transport sub-decision provisional — pending the Phase-0 runtime spike below)

## Context

Today every piece of TfL logic — fetching `api.tfl.gov.uk`, the multi-mode
fan-out, hub-merging, TTL caching, direction inference, severity buckets, the
defensive filters, and the polling stream — lives in the Rust crates
(`tfl-domain`, `tfl-client`, `tfl-cache`, `tfl-board`) and the Tauri command
layer. The SvelteKit webview only renders: it gets data over `invoke()` and the
`board://updated` event.

We are moving all TfL logic into a framework-agnostic TypeScript core under
`web/src/lib/tfl/`, so the frontend fetches TfL directly. The Tauri desktop
shell stays but shrinks to native-only concerns (window chrome, tray,
CoreLocation, auto-update, persistence). The native-SwiftUI migration is paused;
this is the primary direction.

This is a **port (rewrite Rust → TS)**, not a code move — the logic only exists
in Rust. `tubbie-web` is just the marketing site plus a tiny `/pool-keys.json`
endpoint serving public TfL keys.

The crates are **not** being deleted. [`crates-as-public-contract`](./crates-as-public-contract.md)
makes them a SHA-pinned contract for `tubbie-ios`; they stay in the workspace
with all tests and fixtures, and `cargo test --workspace` must stay green.
Desktop only removes the *dependency edges* (at the end of the port).

## Decision

1. **Port incrementally, behind a runtime flag.** A new TS core under
   `web/src/lib/tfl/`, ported crate-by-crate, each phase tested against the
   **same repo-root `fixtures/`** the Rust tests use, then cut over behind
   `USE_TS_TFL` (a `localStorage` toggle, default off). The app ships green at
   every phase.

2. **Transport = plain `globalThis.fetch`.** TfL responds with
   `Access-Control-Allow-Origin: *` and our requests authenticate with an
   `app_key` *query parameter* (not a custom header), so there is no CORS
   preflight. Plain `fetch` keeps the core pure-browser-portable (no Tauri
   dependency in the data layer). **`@tauri-apps/plugin-http` is the
   contingency** if a CORS edge ever appears — so transport stays behind a
   `TflHttp` interface (landing in Phase 2), making the swap a one-file change.

3. **CSP.** `src-tauri/tauri.conf.json` `connect-src` gains
   `https://tubbie.brunobelcastro.com` (for `pool-keys.json`).
   `https://api.tfl.gov.uk` was already allow-listed.

4. **Persistence stays in Rust.** `load_config` / `save_config` / favorites /
   display-prefs / app-key remain over IPC. This keeps the personal app key a
   Rust-only secret and keeps `migrate_legacy_line_ids` the single migration
   site. Config does **not** move to `localStorage`.

5. **Auth via the public pool keys.** The frontend fetches
   `https://tubbie.brunobelcastro.com/pool-keys.json`, validates
   `schema_version == 1` + 32-hex keys, rotates round-robin, and appends
   `app_key`. The keys are already public, so pool-only is sufficient.

## Phase-0 runtime spike (transport verification)

The transport sub-decision is provisional until confirmed from the running
app's webview devtools (cannot be checked headlessly). Expected results:

- `fetch('https://api.tfl.gov.uk/StopPoint/Mode/tube')` → 200, JSON body,
  response header `access-control-allow-origin: *`.
- `fetch('https://tubbie.brunobelcastro.com/pool-keys.json')` → 200 +
  `{ schema_version: 1, keys: [...] }`.
- An `?app_key=...` request issues **no** `OPTIONS` preflight.

_Result: pending — to be recorded here before the flag wiring lands (Phase 5)._

## Consequences

- **New drift risk.** Two implementations of the TfL contract (Rust for iOS, TS
  for desktop). The shared `fixtures/` and `tests/fixtures/hub-vectors.json`
  become the only sync mechanism, so the TS suite asserts against the same files
  the Rust suite does. The TS suite is effectively a 4th consumer of
  `hub-vectors.json`.
- **No iOS submodule bump** is needed for the port itself (crate public surface
  is unchanged). Only editing a shared fixture would require coordination per
  [`crates-as-public-contract`](./crates-as-public-contract.md).
- If the spike surfaces a CORS edge, swapping `FetchTflHttp` for a
  `@tauri-apps/plugin-http` implementation is contained to one file behind the
  `TflHttp` interface; no caller changes.

# ADR: Port TfL logic from Rust into the TypeScript frontend

**Status:** Accepted (transport sub-decision refined by the Phase-0 spike below: TfL works with plain `fetch`; the `pool-keys.json` endpoint needs a CORS fix or must be fetched via `@tauri-apps/plugin-http`)

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

Verified 2026-06-12 via `curl` against the live endpoints (the webview shares the
same network stack, so server-sent CORS headers are authoritative). Tested with
and without an `Origin: http://tauri.localhost` request header.

| Check | Result |
| --- | --- |
| `api.tfl.gov.uk/StopPoint/Mode/tube` | **200**, `content-type: application/json`, **`access-control-allow-origin: *`** ✅ |
| `tubbie.brunobelcastro.com/pool-keys.json` | **200**, valid `{schema_version:1, keys:[6×32-hex]}` — but **NO `access-control-allow-origin` header** on GET or OPTIONS (Vercel route `/api/pool-keys`) ⚠️ |
| `?app_key=…` request shape | simple GET, no custom request headers → **no preflight** triggered ✅ |

**Finding — pool-keys CORS gap.** TfL itself is CORS-open (`ACAO: *`), so the bulk
of the traffic works with plain `fetch`. But `pool-keys.json` returns no `ACAO`
header, so a cross-origin webview `fetch` succeeds at the network layer yet the
browser **blocks JavaScript from reading the body**. This is the exact CORS edge
this ADR anticipated.

**Resolution (decide in Phase 2, when transport lands).** Preferred: add
`Access-Control-Allow-Origin: *` to the `/api/pool-keys` Vercel response in the
`tubbie-web` repo — the keys are already public and the endpoint carries no
session semantics (no `Set-Cookie`, no `Authorization` reflection), so `*` is
correct and exposes nothing CORS wasn't already gating. The browser's model also
forbids combining `*` with `Access-Control-Allow-Credentials: true`, so no
cookie/token-riding read is possible. **Emit the header unconditionally (not
gated on the request's `Origin`)** so a CDN/edge cache can't serve a variant
without it — i.e. avoid a `Vary: Origin` mismatch. Fallback: fetch
`pool-keys.json` through `@tauri-apps/plugin-http` (the named contingency) while
keeping the TfL fetches on plain `fetch`. Either way the `TflHttp` / pool-key
seam isolates the choice to one file.

**Pool-key trust (Phase 2 implementer, note).** The `schema_version == 1` +
32-hex checks are input-*shape* validation, not an authenticity guarantee.
Authenticity rests on HTTPS transport integrity; a compromised `/api/pool-keys`
origin (or DNS/CDN tampering) would hand the client attacker-controlled keys.
This is an **accepted risk** in the current threat model — the keys carry no
privilege (anonymous-tier TfL access), and a MITM able to tamper with the key
list can equally tamper with the TfL responses themselves. No SRI/out-of-band
check is planned. Also note that moving the fetch into the webview makes the
endpoint URL and rotation logic visible in devtools/network inspection (the keys
are already public, so the only exposure is endpoint hammering) — **confirm the
rate-limit posture of `/api/pool-keys` before Phase 5 wiring.**

**Still unverified (needs the running app):** that WKWebView's CSP actually
permits the `connect-src` to `tubbie.brunobelcastro.com` at runtime — `curl`
cannot exercise webview CSP enforcement. Confirm from devtools before Phase 5
wiring.

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

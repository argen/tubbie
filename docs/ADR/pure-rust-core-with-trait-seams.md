# ADR: Pure Rust Core with Trait Seams

**Status:** Accepted

## Context

The business logic (fetching arrivals, filtering by line/direction, formatting
for display) could live in the frontend (TypeScript) or the Rust backend. Both
sides need to agree on data shapes.

Putting business logic in TypeScript makes it hard to test without a running
Tauri process, and ties tests to the webview environment.

## Decision

All domain logic lives in pure Rust crates with zero I/O at the library level.
I/O (HTTP, persistence) is injected via traits:

- `TflHttp` trait — two impls: `ReqwestTflHttp` (live) and `FixtureTflHttp` (test)
- `Clock` trait — `RealClock` uses `Utc::now()`, `FakeClock` used in tests

Three crates:
- `tfl-domain` — pure types, zero I/O, zero deps beyond serde
- `tfl-client` — HTTP trait + impls
- `tfl-board` — orchestration: polling, filtering, formatting

Tauri commands are thin wrappers in `src-tauri` that delegate to `tfl-board`.

## Consequences

- Business logic is testable with `cargo test` alone — no Tauri, no browser.
- Fixture-replay testing (ADR #4) becomes straightforward.
- The TypeScript frontend is a pure view layer: it receives `Board` structs and
  renders them. It does not contain business logic.
- Adding a web companion later is clean: the same Rust logic can be compiled to
  WASM or exposed via a separate HTTP server.

## Status

Accepted.

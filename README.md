# tubbie

TfL tube arrivals dot-matrix board — Tauri v2 + SvelteKit + Rust.

## Prerequisites

- [Rust](https://rustup.rs/) (stable, via `rust-toolchain.toml`)
- Node.js v24 (see `web/.nvmrc`)
- [just](https://github.com/casey/just) — task runner
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform

## Self-verify

```sh
cd web && npm install
cd ..
just verify
```

Both the Rust gate (`cargo fmt --check`, `cargo clippy`, `cargo test`) and the
web gate (`lint`, `format:check`, `typecheck`, `test`) must be green.

### Running live tests

Live integration tests hit `api.tfl.gov.uk` and are **not run in CI**. To run
them locally:

```sh
# Optional but recommended: set your TfL app key to avoid rate-limits.
export TFL_APP_KEY=your_key_here

just verify-live
```

This runs `cargo test --workspace --features tfl-client/live`. Tests
automatically skip (and pass) when network connectivity is unavailable, so
they never cause unexpected failures on offline machines.

## Dev server

```sh
just dev
```

> **Note:** `just dev` is a placeholder until M5/M6 when the Tauri shell and
> SvelteKit frontend are wired together.

## Regenerating fixtures

Fixtures are committed TfL API responses used for offline, deterministic tests.
They live in `fixtures/{endpoint}/{id}.json` alongside a `{id}.meta.json` sidecar
recording when each fixture was captured and from what URL (`app_key` is always
stripped before writing).

To refresh:

```sh
just record-fixtures
```

This hits the live TfL API (anonymous access, no key needed) for:
- `arrivals/{id}.json` — 4 representative stations (Belsize Park, King's Cross, Bank, Oxford Circus)
- `line-status/tube.json` — all tube line statuses
- `stop-points/tube.json` — searchable station list (~1 MB)

Refresh before each milestone that touches domain types, and commit the updated
fixtures alongside the code changes.

## Architecture

See [`docs/README.md`](docs/README.md) for conceptual documentation and
[`docs/ADR/README.md`](docs/ADR/README.md) for architectural decisions.

## Running the desktop app

### Prerequisites

- Rust (stable, via `rustup`) — see `rust-toolchain.toml`
- Node 24 — see `web/.nvmrc`
- `just` 1.x — `cargo install just`
- Tauri CLI v2 — `cargo install tauri-cli@^2`

### Development mode

```sh
cd web && npm install  # first time only
just dev               # starts Vite at :5173 + tauri dev concurrently
```

`tauri dev` reads `src-tauri/tauri.conf.json`, loads the Vite dev server as
the webview, and hot-reloads Rust when you change backend code.

### Running tests (no display required)

```sh
just verify-rust       # fmt + clippy + cargo test --workspace
```

All Tauri command tests run headlessly via fixture files — no network, no
display server, no Tauri runtime required.

### Optional: TfL API key

Anonymous access is the default (50 requests/min, comfortable for personal
use). To use your own key:

1. Register at https://api-portal.tfl.gov.uk
2. Set `TFL_APP_KEY=<your-key>` in your environment, **or** enter it in the
   app's Settings screen (persisted via `tauri-plugin-store`; takes effect
   after a restart).

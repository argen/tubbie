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

## Dev server

```sh
just dev
```

> **Note:** `just dev` is a placeholder until M5/M6 when the Tauri shell and
> SvelteKit frontend are wired together.

## Architecture

See [`docs/README.md`](docs/README.md) for conceptual documentation and
[`docs/ADR/README.md`](docs/ADR/README.md) for architectural decisions.

# ADR: Strict Tooling Baseline

**Status:** Accepted

## Context

A TfL arrivals app built with Tauri + SvelteKit + Rust requires consistent
developer experience across both the JS/TS frontend and the Rust core. Without
a shared, enforced tooling baseline, code quality diverges quickly as the
project grows or new agents/contributors join.

The project also uses AI agents (Opus/Sonnet) for milestone implementation.
Agents benefit from a deterministic self-check gate (`just verify`) so they
can confirm correctness before handing back.

## Decision

Apply a strict tooling harness as milestone M-1 — before any feature code.

**Frontend (`web/`):**
- Node v24 pinned via `.nvmrc` + `engine-strict=true`
- ESLint v10 flat config with `typescript-eslint` type-aware strict rules
- Prettier 3 + `prettier-plugin-svelte`
- TypeScript strict mode + `noUncheckedIndexedAccess` + `exactOptionalPropertyTypes` + `verbatimModuleSyntax`
- Vitest node-env default; DOM tests opt in via `// @vitest-environment happy-dom`
- `simple-git-hooks` + `lint-staged` for pre-commit enforcement

**Rust side:**
- `rust-toolchain.toml` pins stable + `rustfmt` + `clippy`
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`

**Orchestration:**
- `justfile` at repo root; `just verify` is the single gate entry point

## Consequences

- Every milestone is deliverable only when `just verify` is green — no exceptions.
- Agents cannot soften rules to make the gate pass; they must fix the code.
- Initial setup cost is ~1 milestone, but pays dividends for the entire project lifecycle.
- `exactOptionalPropertyTypes` + SvelteKit may produce friction when kit-generated
  types use `T | undefined` loosely — track and resolve per occurrence.

## Status

Accepted — implemented in M-1.

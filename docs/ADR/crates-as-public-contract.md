# ADR: `crates/tfl-*` are now a public contract

**Status:** Accepted

## Context

Tubbie's `crates/tfl-domain`, `crates/tfl-client`, and `crates/tfl-board` were
designed under the pure-Rust-core ADR as cleanly factored, dependency-free
modules — but until now their only consumer was `src-tauri/` in this same
workspace. Internal-only consumers means Cargo path deps are sufficient and any
breaking change can be made in lockstep.

The iOS port [`argen/tubbie-ios`](https://github.com/argen/tubbie-ios) consumes
these three crates via a SHA-pinned git submodule and Cargo path deps. The
shape of `BoardConfig`, `Board`, `BoardService::stream`, the `TflHttp` /
`Clock` / `ConfigStore` traits, and every public type they touch, is now
load-bearing for two independent shells.

## Decision

`crates/tfl-domain`, `crates/tfl-client`, and `crates/tfl-board` are treated as
a public contract. Breaking changes to their public surface require:

1. Coordinating with the iOS shell — typically a paired PR pair: change here,
   then bump the submodule SHA in `tubbie-ios`.
2. A justification in the PR description that the change is genuinely needed
   (i.e. it pays for the cross-repo work).
3. The full commit-blocking test table in this `CLAUDE.md` green before the PR
   merges. Submodule pinning in `tubbie-ios` happens only against a known-green
   tubbie SHA — that is the contract.

There is no published version number. The submodule pin is the wire. Semver
rules apply in spirit: the iOS shell expects breaking changes to be rare and
deliberate, not silent.

## Consequences

- Refactors inside `crates/tfl-*` that don't touch public symbols stay free.
- Renames, signature changes, and type changes to public symbols become
  two-PR-and-a-bump events. The `tubbie-ios` `bump-core` Justfile recipe
  enforces the gate.
- The `src-tauri/` shell is not part of this contract — it can change shape
  freely without notifying anyone. Only the three core crates are pinned.
- This ADR does not create a release process, a changelog, or version
  numbers. It only documents the change in social contract: these three crates
  now have an external consumer.

## Status

Accepted.

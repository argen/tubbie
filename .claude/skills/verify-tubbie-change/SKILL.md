---
name: verify-tubbie-change
description: Run when wrapping up a code change in the tubbie repo — before reporting work as complete, before opening a PR, or when the user says "done", "ready to commit", "ready to PR", "ship it", or "is this ready". Runs `cargo test --workspace`, `cd web && npm test`, and `cargo clippy --workspace --all-targets -- -D warnings`; performs a self-review pass on the diff; points at the right manual smoke checklist in `docs/manual-smoke.md` based on what was modified; and flags iOS submodule impact when public surface of `crates/tfl-domain`, `crates/tfl-client`, or `crates/tfl-board` changed. Use proactively after any non-trivial edit to `src-tauri/`, `crates/tfl-*`, or `web/`.
---

# Verify a tubbie change

Work through the steps in order. Stop and report at the first failure
— do not paper over a failing test or warning to keep the workflow
moving.

## 1. Inspect what changed

```bash
git status
git diff --stat
git diff
```

Note which areas were touched — this drives the manual-smoke section
to run, and whether iOS impact applies:

- `src-tauri/` (Tauri commands, lib.rs, AppState wiring)
- `crates/tfl-domain/`, `crates/tfl-client/`, `crates/tfl-board/`
- `web/` (Svelte, stores, types)
- Public crate surface (`pub` fns/types/modules in `crates/tfl-*`)

## 2. Run the automated suite

In parallel:

- `cargo test --workspace` — must be green.
- `cd web && npm test` — must be green.
- `cargo clippy --workspace --all-targets -- -D warnings` — no
  warnings.

If you added or modified a regression test for a new failure mode,
verify it would actually fail without the fix: revert the fix
locally, watch the test go red, then restore.

## 3. Self-review the diff

Read the full diff again with fresh eyes. Look for:

- Code that compiles/passes tests but doesn't do what the task
  required.
- Hidden assumptions about caches, ordering, or thread context (esp.
  Cocoa main-thread invariants #8 and #9).
- Backwards-compat shims, `_unused` renames, or dead-code comments
  that should just be deleted.
- New `unwrap()` / `expect()` on paths that can fail in production.
- Any change to `BoardConfig`, `AppState`, `cfg_tx` wiring,
  `apply_filters`, or `BoardService::refresh` — verify the relevant
  invariant in `CLAUDE.md` still holds.

If the diff is large or touches the stream/config pipeline, consider
spawning a `general-purpose` review subagent for an independent pass.

## 4. Manual smoke (only for the change types listed)

Open `docs/manual-smoke.md` and run the section matching what was
modified. **Don't skip — automated tests don't catch resize, tray, or
stream-respawn regressions.**

- Stream / config pipeline → "Stream / config changes"
- Display-mode (`apply_display_mode_effects`, `save_display_mode`,
  `display_mode` lock) → "Display-mode changes"
- Board resize / `linesGrouped` (`apply_board_size`,
  `Board.svelte::pickBoardSize`) → "Adaptive resize / line-grouped
  layout"

Pure refactors with no behavioural change still run the automated
suite, but skip manual smoke.

## 5. iOS submodule impact

If the diff modified `pub` items in `crates/tfl-domain`,
`crates/tfl-client`, or `crates/tfl-board`, the iOS shell at
`~/Sites/tubbie-ios` consumes those crates as a SHA-pinned submodule.
Per `docs/ADR/crates-as-public-contract.md`:

- A breaking public-API change requires a paired PR + submodule bump
  in `tubbie-ios`. Flag it in the PR description so it isn't merged
  alone.
- An internal refactor (no public-surface delta) needs nothing.

Quick check:

```bash
git diff -- 'crates/tfl-*/src/**/*.rs' | grep -E '^\+.*\bpub\b|^-.*\bpub\b'
```

If empty, no public-surface change. If non-empty, classify each
line — some `pub` deltas are internal-only (e.g. inside a non-`pub`
module) and don't break the contract.

## 6. Report

Tell the user:

- Test status (cargo / npm / clippy — pass / fail).
- Which manual-smoke section was run (or "skipped — pure refactor").
- iOS impact: none / breaking-change-needs-paired-PR.
- Anything you'd want a reviewer to look at twice.

Keep it tight — two or three sentences plus a short bullet list.

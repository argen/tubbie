#!/usr/bin/env bash
# Refuse any commit on this branch that touches the tfl-* crates.
#
# tubbie-ios consumes `crates/tfl-domain`, `tfl-client`, `tfl-board`
# (transitively also `tfl-cache`) via a SHA-pinned submodule.
# Changes to those crates' source must go through the cross-repo
# "bump-core" workflow (see CLAUDE.md), not slip into a desktop PR.
#
# Usage:
#   scripts/check-tfl-crates-frozen.sh [base-ref]
#
# Default base is `origin/main`. Exits 0 if no tfl-* files changed
# vs base, 1 if any did. The pre-push hook runs this before pushing
# any branch.
set -euo pipefail

BASE="${1:-origin/main}"

# git diff with `...` (merge-base) reports files changed by THIS branch
# only, not the union of everything on the base since we forked. That
# keeps us honest if `main` itself has been updated under our feet.
TOUCHED="$(git diff --name-only "${BASE}...HEAD" \
            | grep -E '^crates/(tfl-domain|tfl-client|tfl-board|tfl-cache)/' \
            || true)"

if [[ -n "$TOUCHED" ]]; then
  cat >&2 <<EOF
error: this branch touches tfl-* crates, which tubbie-ios submodules:
${TOUCHED}

Coordinate via the bump-core dance from CLAUDE.md, not a desktop PR.
EOF
  exit 1
fi

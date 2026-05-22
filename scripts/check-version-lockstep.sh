#!/usr/bin/env bash
# Refuse a state where `src-tauri/tauri.conf.json:.version` and
# `src-tauri/Cargo.toml:[package].version` disagree.
#
# Tauri's signed-update path keys everything off `tauri.conf.json`,
# but `cargo` keys off `Cargo.toml`. A drift silently produces an
# installer whose bundle version doesn't match the manifest version
# the updater checks against — auto-update breaks for every user.
#
# Usage:
#   scripts/check-version-lockstep.sh
#
# Exits 0 if they match, 1 if they don't.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONF="${ROOT}/src-tauri/tauri.conf.json"
CARGO="${ROOT}/src-tauri/Cargo.toml"
LOCK="${ROOT}/Cargo.lock"

conf_version="$(python3 -c "import json,sys; print(json.load(open('${CONF}'))['version'])")"
# Take the first version= line under [package]; Cargo.toml's [package]
# block is always the first, and `version` is keyed once per block.
cargo_version="$(awk '/^\[package\]/ {flag=1; next} /^\[/ {flag=0} flag && /^version = /' "${CARGO}" \
                  | head -1 \
                  | sed -E 's/version = "([^"]+)".*/\1/')"
# Cargo.lock's [[package]] entry for `tubbie`. The version line is the
# next non-blank line after `name = "tubbie"`. The lockfile MUST agree
# or the release artifacts will carry an inconsistent self-reported
# version (Tauri reads conf, cargo reads Cargo.toml, and the build
# graph keys off Cargo.lock — drift surfaces as a stale `getVersion()`
# in the installed binary).
lock_version="$(awk '/^name = "tubbie"$/ {getline; print; exit}' "${LOCK}" \
                  | sed -E 's/version = "([^"]+)".*/\1/')"

if [[ "$conf_version" != "$cargo_version" ]] \
   || [[ "$conf_version" != "$lock_version" ]]; then
  cat >&2 <<EOF
error: version drift between bundle / crate / lockfile manifests:
  src-tauri/tauri.conf.json  version: ${conf_version}
  src-tauri/Cargo.toml       version: ${cargo_version}
  Cargo.lock (tubbie)        version: ${lock_version}

Run \`just bump <new-version>\` to keep them in lockstep.
EOF
  exit 1
fi

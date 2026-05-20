#!/usr/bin/env bash
# Set `version` in both `src-tauri/tauri.conf.json` and
# `src-tauri/Cargo.toml` to the same value, in one atomic edit.
#
# Usage:
#   scripts/bump-version.sh 0.1.0
#   scripts/bump-version.sh 0.1.1-rc.1
#
# The version is validated against a permissive semver regex (allows
# pre-release tags). The script runs `check-version-lockstep.sh` at
# the end to confirm the edits agree.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <semver>" >&2
  exit 2
fi
NEW="$1"

# Permissive semver: MAJOR.MINOR.PATCH with optional pre-release.
if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "error: ${NEW} is not a valid semver (e.g. 0.1.0 or 0.1.1-rc.1)" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONF="${ROOT}/src-tauri/tauri.conf.json"
CARGO="${ROOT}/src-tauri/Cargo.toml"

python3 - "${CONF}" "${NEW}" <<'PY'
import json, sys
path, new = sys.argv[1], sys.argv[2]
with open(path) as fh:
    data = json.load(fh)
data['version'] = new
with open(path, 'w') as fh:
    json.dump(data, fh, indent=2)
    fh.write('\n')
PY

# Only edit the version under the first [package] block (Cargo.toml has
# one [package] section; this is robust to other 'version =' lines
# appearing inside dependency tables).
python3 - "${CARGO}" "${NEW}" <<'PY'
import re, sys
path, new = sys.argv[1], sys.argv[2]
with open(path) as fh:
    txt = fh.read()
def repl(match):
    return f'{match.group(1)}version = "{new}"\n'
# Match the first 'version = "..."' line after [package] but before any
# next [section] header.
pattern = re.compile(
    r'(\[package\][^\[]*?\n)version\s*=\s*"[^"]+"\s*\n',
    re.DOTALL,
)
new_txt, n = pattern.subn(repl, txt, count=1)
if n != 1:
    sys.exit(f"error: could not find [package].version line in {path}")
with open(path, 'w') as fh:
    fh.write(new_txt)
PY

"${ROOT}/scripts/check-version-lockstep.sh"
echo "bumped to ${NEW} (tauri.conf.json + src-tauri/Cargo.toml)"

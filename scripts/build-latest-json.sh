#!/usr/bin/env bash
# Emit a Tauri v2 updater manifest (`latest.json`) for the just-built
# release artifacts. Reads the version from `tauri.conf.json`, picks
# up the `.app.tar.gz.sig` signature file produced by
# `cargo tauri build`, and writes a manifest pointing at the
# GitHub Release asset URLs for the supplied tag.
#
# Usage:
#   scripts/build-latest-json.sh v0.1.0 > target/aarch64-apple-darwin/release/bundle/macos/latest.json
#
# Tauri's updater reads this manifest from the URL configured in
# `tauri.conf.json:plugins.updater.endpoints[0]`. The `signature`
# value is the contents of the `.app.tar.gz.sig` file, inlined as
# a literal string.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <tag>" >&2
  exit 2
fi
TAG="$1"
VERSION="${TAG#v}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE_DIR="${ROOT}/target/aarch64-apple-darwin/release/bundle/macos"

SIG_PATH="$(ls "${BUNDLE_DIR}"/*.app.tar.gz.sig 2>/dev/null | head -1)"
TARBALL_PATH="$(ls "${BUNDLE_DIR}"/*.app.tar.gz 2>/dev/null | grep -v '\.sig$' | head -1)"

if [[ -z "${SIG_PATH:-}" || -z "${TARBALL_PATH:-}" ]]; then
  echo "error: missing .app.tar.gz or .app.tar.gz.sig in ${BUNDLE_DIR}" >&2
  echo "       Did \`cargo tauri build\` complete with createUpdaterArtifacts=true?" >&2
  exit 1
fi

SIG_CONTENTS="$(cat "${SIG_PATH}")"
TARBALL_BASENAME="$(basename "${TARBALL_PATH}")"
PUB_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# Release notes: prefer CHANGELOG-<tag>.md if present, fall back to an
# empty body. Notes are markdown; Tauri's plugin doesn't render them
# itself (we surface release notes via a "Release notes →" link in
# Settings instead, per the UI spec in PR-E).
NOTES_FILE="${ROOT}/CHANGELOG-${TAG}.md"
if [[ -r "${NOTES_FILE}" ]]; then
  NOTES="$(cat "${NOTES_FILE}")"
else
  NOTES=""
fi

URL="https://github.com/argen/tubbie/releases/download/${TAG}/${TARBALL_BASENAME}"

# Use python3 for JSON serialisation — quoting via printf is too risky
# for arbitrary release-note strings (newlines, quotes, backslashes).
python3 - "${VERSION}" "${NOTES}" "${PUB_DATE}" "${SIG_CONTENTS}" "${URL}" <<'PY'
import json, sys
version, notes, pub_date, signature, url = sys.argv[1:6]
manifest = {
    "version": version,
    "notes": notes,
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {
            "signature": signature,
            "url": url,
        }
    },
}
json.dump(manifest, sys.stdout, indent=2)
sys.stdout.write("\n")
PY

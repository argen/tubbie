#!/usr/bin/env bash
# Refuse to start a release run if any of the following is true:
#
#  - working tree is dirty (untracked or modified files)
#  - current branch is not `main`
#  - the requested tag's version doesn't match
#    `src-tauri/tauri.conf.json:.version`
#  - required signing env vars are unset
#  - we already have a tag with this name (no clobbering past releases)
#  - notarytool can't authenticate with the API key in NOTARY_*
#
# Usage:
#   scripts/preflight.sh v0.1.0
#
# Run from `just release <tag>` before any cargo work happens.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <tag>" >&2
  exit 2
fi
TAG="$1"
EXPECTED="${TAG#v}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

fail() { echo "preflight: $*" >&2; exit 1; }

# 1. Clean working tree
if [[ -n "$(git status --porcelain)" ]]; then
  fail "working tree is dirty — commit or stash first"
fi

# 2. On main
branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != "main" ]]; then
  fail "not on main (you're on ${branch}) — releases cut from main only"
fi

# 3. Local main == origin/main (refuse if behind or ahead)
git fetch --quiet origin main
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
  fail "main diverges from origin/main — pull/push before releasing"
fi

# 4. Version matches tag
actual="$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
if [[ "$actual" != "$EXPECTED" ]]; then
  fail "tauri.conf.json version is ${actual}, expected ${EXPECTED} for tag ${TAG}"
fi
"${ROOT}/scripts/check-version-lockstep.sh"

# 5. Tag doesn't already exist locally or remotely
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  fail "tag ${TAG} already exists locally"
fi
if git ls-remote --exit-code --tags origin "${TAG}" >/dev/null 2>&1; then
  fail "tag ${TAG} already exists on origin"
fi

# 6. Required signing + notarization env vars
for var in APPLE_SIGNING_IDENTITY APPLE_TEAM_ID TAURI_SIGNING_PRIVATE_KEY \
           NOTARY_KEY_PATH NOTARY_KEY_ID NOTARY_ISSUER; do
  if [[ -z "${!var:-}" ]]; then
    fail "${var} is not set — source your .envrc first"
  fi
done
if [[ ! -r "${TAURI_SIGNING_PRIVATE_KEY}" ]]; then
  fail "TAURI_SIGNING_PRIVATE_KEY=${TAURI_SIGNING_PRIVATE_KEY} is not readable"
fi
if [[ ! -r "${NOTARY_KEY_PATH}" ]]; then
  fail "NOTARY_KEY_PATH=${NOTARY_KEY_PATH} is not readable"
fi

# 7. Signing identity is present in login keychain
if ! security find-identity -p codesigning -v 2>/dev/null \
     | grep -q "${APPLE_SIGNING_IDENTITY}"; then
  fail "signing identity '${APPLE_SIGNING_IDENTITY}' not in login keychain"
fi

# 8. notarytool API key authenticates against Apple
if ! xcrun notarytool history \
       --key "${NOTARY_KEY_PATH}" \
       --key-id "${NOTARY_KEY_ID}" \
       --issuer "${NOTARY_ISSUER}" >/dev/null 2>&1; then
  fail "notarytool API key auth failed — check NOTARY_KEY_PATH / NOTARY_KEY_ID / NOTARY_ISSUER in .envrc"
fi

echo "preflight OK for ${TAG}"

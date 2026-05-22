#!/usr/bin/env bash
# Submit a .zip to Apple's notary service and poll until terminal,
# surviving transient network drops that would kill `notarytool --wait`.
#
# Background: during the Phase 3 dry-run we observed `xcrun notarytool
# submit --wait` aborting with NSURLErrorDomain -1009 ("Internet
# connection appears to be offline") when the local Wi-Fi blinked.
# Apple-side, the submission keeps processing — but the local recipe
# aborts. This wrapper decouples the submit from the wait and polls
# resiliently.
#
# Usage:
#   scripts/notarize-submit-and-wait.sh <path-to-zip>
#
# Requires in env:
#   NOTARY_KEY_PATH   path to App Store Connect API .p8
#   NOTARY_KEY_ID     10-char Key ID
#   NOTARY_ISSUER     issuer UUID
#
# Optional overrides (env):
#   NOTARY_POLL_INTERVAL_SECS    default 30
#   NOTARY_MAX_WAIT_SECS         default 1800 (30 min)
#
# Exit codes:
#   0  - Accepted
#   1  - Invalid / Rejected (terminal failure from Apple)
#   2  - usage / configuration error
#   3  - timed out (still In Progress past NOTARY_MAX_WAIT_SECS;
#                   submission is on Apple's side, re-run with `query`)
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <path-to-zip> | query <submission-id>" >&2
  exit 2
fi

for var in NOTARY_KEY_PATH NOTARY_KEY_ID NOTARY_ISSUER; do
  if [[ -z "${!var:-}" ]]; then
    echo "error: ${var} is not set — source .envrc first" >&2
    exit 2
  fi
done

POLL_INTERVAL="${NOTARY_POLL_INTERVAL_SECS:-30}"
MAX_WAIT="${NOTARY_MAX_WAIT_SECS:-1800}"

notary() {
  xcrun notarytool "$@" \
    --key "${NOTARY_KEY_PATH}" \
    --key-id "${NOTARY_KEY_ID}" \
    --issuer "${NOTARY_ISSUER}"
}

# `query` mode lets a human re-attach to a submission that timed out
# locally (e.g. laptop slept). Useful when re-running the staple step
# after a previous run hit MAX_WAIT.
if [[ "$1" == "query" ]]; then
  if [[ $# -ne 2 ]]; then
    echo "usage: $0 query <submission-id>" >&2
    exit 2
  fi
  SUB_ID="$2"
else
  ZIP="$1"
  if [[ ! -r "${ZIP}" ]]; then
    echo "error: ${ZIP} not readable" >&2
    exit 2
  fi
  echo "[notarize] submitting ${ZIP}…"
  # Submit *without* --wait. We do our own polling so a network blink
  # doesn't kill the whole pipeline.
  SUBMIT_OUT="$(notary submit "${ZIP}" 2>&1)"
  echo "${SUBMIT_OUT}"
  SUB_ID="$(echo "${SUBMIT_OUT}" | awk '/^[[:space:]]*id: / {print $2; exit}')"
  if [[ -z "${SUB_ID}" ]]; then
    echo "error: could not parse submission id from notarytool output" >&2
    exit 1
  fi
  echo "[notarize] submission id: ${SUB_ID}"
fi

START="$(date +%s)"
while true; do
  ELAPSED=$(( $(date +%s) - START ))
  # `notarytool info` can transiently fail on network blips — capture
  # exit code but DON'T propagate it. If we got nothing parseable,
  # treat as "still In Progress" and keep polling.
  if INFO_OUT="$(notary info "${SUB_ID}" 2>&1)"; then
    STATUS="$(echo "${INFO_OUT}" | awk '/^[[:space:]]*status:/ {print $2; exit}')"
  else
    STATUS=""
  fi
  printf "[notarize] elapsed=%4ds status=%s\n" "${ELAPSED}" "${STATUS:-<network-error>}"

  case "${STATUS}" in
    Accepted)
      echo "[notarize] ✓ Accepted"
      exit 0
      ;;
    Invalid|Rejected)
      echo "[notarize] ✗ ${STATUS} — fetching log:" >&2
      notary log "${SUB_ID}" 2>&1 || true
      exit 1
      ;;
    "In"|"")
      # "In Progress" is parsed as "In" by awk (first whitespace-delimited
      # token after `status:`). Empty status means transient query error.
      if (( ELAPSED >= MAX_WAIT )); then
        echo "[notarize] giving up after ${MAX_WAIT}s — submission ${SUB_ID} is still on Apple's side" >&2
        echo "[notarize] re-attach via: $0 query ${SUB_ID}" >&2
        exit 3
      fi
      sleep "${POLL_INTERVAL}"
      ;;
    *)
      # Unknown status word — log it and keep polling. Better to
      # over-wait than to fail on an unexpected Apple-side string.
      echo "[notarize] unexpected status '${STATUS}' — continuing to poll" >&2
      sleep "${POLL_INTERVAL}"
      ;;
  esac
done

#!/usr/bin/env bash
# End-to-end proof that `lurien serve` drives the lurien engine over the exact
# HTTP path an agent runtime uses.
#
# Asserts, through the wire protocol:
#   1. /v1/health reports stealth_engine=lurien and captcha_solve=true
#   2. launch + goto reach a page and report its URL
#   3. navigator.webdriver is false in the live page (stealth engine, not stock)
#   4. a second context runs concurrently with the first
#   5. state snapshot round-trips: state-set restores what state captured
#   6. close releases the context and health counts it down
#
# Usage:
#   LURIEN_BIN=<lurien engine> LURIEN_SERVE=<path/to/lurien> \
#     bash software/browser/lurien/tests/e2e_serve.sh
#
# Skips (exit 0) when the engine or the binary is missing, so CI without the
# multi-GB engine build is not a red run.
set -u

ENGINE_BIN="${LURIEN_BIN:-}"
# The host owns the target directory; ask cargo rather than hardcoding a path.
TARGET_DIR="${CARGO_TARGET_DIR:-$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
SERVE_BIN="${LURIEN_SERVE:-${TARGET_DIR}/debug/lurien}"
PORT="${LURIEN_SERVE_PORT:-7473}"
BASE="http://127.0.0.1:${PORT}"

if [ -z "${ENGINE_BIN}" ] || [ ! -x "${ENGINE_BIN}" ]; then
  echo "SKIP: LURIEN_BIN unset or not executable"
  exit 0
fi
if [ ! -x "${SERVE_BIN}" ]; then
  echo "SKIP: ${SERVE_BIN} not built (cargo build -p lurien-driver)"
  exit 0
fi

LOG="$(mktemp /tmp/lurien-serve-e2e-log.XXXXXX)"
PROFILE_A="$(mktemp -d /tmp/lurien-serve-e2e-a.XXXXXX)"
PROFILE_B="$(mktemp -d /tmp/lurien-serve-e2e-b.XXXXXX)"
CTX_A="e2e-a-$$"
CTX_B="e2e-b-$$"
FAILED=0

cleanup() {
  [ -n "${SERVE_PID:-}" ] && kill "${SERVE_PID}" 2>/dev/null
  rm -rf "${PROFILE_A}" "${PROFILE_B}"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; FAILED=1; }

post() {
  curl -s --max-time 180 -H 'Content-Type: application/json' -d "$1" \
    "${BASE}/v1/browser/command"
}

cmd() {
  local context="$1" command="$2" extra="${3:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"${command}\",\"browser_context_id\":\"${context}\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "${extra}" ]; then body="${body},${extra}"; fi
  post "${body}}"
}

LURIEN_BIN="${ENGINE_BIN}" LURIEN_SERVE_BIND="127.0.0.1:${PORT}" \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "${SERVE_BIN}" serve >"${LOG}" 2>&1 &
SERVE_PID=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "${BASE}/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "${LOG}"; exit 1; }

# 1. posture
HEALTH="$(curl -s --max-time 10 "${BASE}/v1/health")"
echo "${HEALTH}" | grep -q '"stealth_engine":"lurien"' || fail "health does not report the lurien engine: ${HEALTH}"
echo "${HEALTH}" | grep -q '"captcha_solve":true' || fail "health lost the captcha_solve capability"
echo "${HEALTH}" | grep -q '"webdriver_masked":true' || fail "health does not report webdriver masked"

# 2. launch and navigate
OUT="$(cmd "${CTX_A}" launch "\"profile_dir\":\"${PROFILE_A}\",\"url\":\"about:blank\"")"
echo "${OUT}" | grep -q '"success":true' || fail "launch failed: ${OUT}"

OUT="$(cmd "${CTX_A}" goto "\"url\":\"data:text/html,<title>lurien</title><input id=probe>\"")"
echo "${OUT}" | grep -q '"success":true' || fail "goto failed: ${OUT}"

# 3. the engine is the stealth engine, in the live page
OUT="$(cmd "${CTX_A}" execute_js "\"args\":{\"code\":\"String(navigator.webdriver)\"}")"
echo "${OUT}" | grep -q 'false' || fail "navigator.webdriver is not false: ${OUT}"

# 4. a second context, concurrently
OUT="$(cmd "${CTX_B}" launch "\"profile_dir\":\"${PROFILE_B}\",\"url\":\"about:blank\"")"
echo "${OUT}" | grep -q '"success":true' || fail "second context failed to launch: ${OUT}"
LIST="$(cmd "${CTX_A}" list_contexts)"
echo "${LIST}" | grep -q '"count":2' || fail "both contexts should be open: ${LIST}"

# 5. state round-trip
OUT="$(cmd "${CTX_A}" goto "\"url\":\"https://example.com/\"")"
echo "${OUT}" | grep -q '"success":true' || fail "navigate to example.com failed: ${OUT}"
cmd "${CTX_A}" execute_js "\"args\":{\"code\":\"(() => { localStorage.setItem('e2e','kept'); return 'set'; })()\"}" >/dev/null
SNAP="$(cmd "${CTX_A}" get_state)"
echo "${SNAP}" | grep -q '"success":true' || fail "get_state failed: ${SNAP}"
echo "${SNAP}" | grep -q 'kept' || fail "state snapshot lost the stored value: ${SNAP}"
cmd "${CTX_A}" clear_state >/dev/null
GONE="$(cmd "${CTX_A}" execute_js "\"args\":{\"code\":\"String(localStorage.getItem('e2e'))\"}")"
echo "${GONE}" | grep -q 'null' || fail "clear_state left the value behind: ${GONE}"

# 6. close
OUT="$(cmd "${CTX_B}" close)"
echo "${OUT}" | grep -q '"closed":true' || fail "close did not release the context: ${OUT}"
OUT="$(cmd "${CTX_A}" close)"
echo "${OUT}" | grep -q '"closed":true' || fail "close did not release the context: ${OUT}"
HEALTH="$(curl -s --max-time 10 "${BASE}/v1/health")"
echo "${HEALTH}" | grep -q '"active_browser_contexts":0' || fail "health still counts a closed context: ${HEALTH}"

if [ "${FAILED}" -ne 0 ]; then
  echo "--- lurien serve log ---"
  cat "${LOG}"
  exit 1
fi
echo "PASS: lurien serve drives the lurien engine over the wire protocol"

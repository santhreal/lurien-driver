#!/usr/bin/env bash
# Session lifecycle on the real engine: what the server does with a browser
# nobody is driving any more.
#
# Three claims:
#
#   1. A live session is described honestly: launched, with its age, its idle
#      time, its URL, and how long it has before the server closes it.
#   2. A session nobody touches is closed on its own. A client that dies without
#      sending close does not leak an engine for the life of the server.
#   3. Driving a session keeps it alive, and an explicit close still works and is
#      reported.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_sessions.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7484}"
base="http://127.0.0.1:$port"
work="$(mktemp -d)"
idle_ms=6000
failed=0

if [ -z "$engine" ] || [ ! -x "$engine" ]; then
  echo "SKIP: LURIEN_BIN unset or not executable"
  exit 0
fi
if [ ! -x "$serve" ]; then
  echo "SKIP: $serve not built (cargo build -p lurien-driver)"
  exit 0
fi

cleanup() {
  [ -n "${serve_pid:-}" ] && kill "$serve_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; failed=1; }

cmd() {
  local command="$1" ctx="$2" extra="${3:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$ctx\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  curl -s --max-time 180 -H 'Content-Type: application/json' -d "$body}" "$base/v1/browser/command"
}

# One field out of the sessions array, by context name.
row_field() {
  local reply="$1" ctx="$2" field="$3"
  printf '%s' "$reply" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
ctx, field = sys.argv[1], sys.argv[2]
for row in reply.get("metadata", {}).get("sessions", []):
    if row.get("browser_context_id") == ctx:
        print(row.get(field, ""))
        break
' "$ctx" "$field"
}

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_SESSION_IDLE_MS="$idle_ms" \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }
grep -q "closing sessions idle for ${idle_ms}ms" "$work/serve.log" \
  || fail "the server did not report its idle deadline: $(cat "$work/serve.log")"

# Phase 1: a launched session describes itself.
reply="$(cmd launch abandoned "\"profile_dir\":\"$work/abandoned\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }
list="$(cmd sessions abandoned)"
echo "$list" | grep -q '"count":1' || fail "one session should be open: $list"
[ "$(row_field "$list" abandoned state)" = "launched" ] \
  || fail "a session with an engine must not report itself as named only: $list"
age="$(row_field "$list" abandoned age_ms)"
[ -n "$age" ] && [ "$age" -ge 0 ] || fail "the session list must report an age: $list"
left="$(row_field "$list" abandoned reap_in_ms)"
[ -n "$left" ] && [ "$left" -le "$idle_ms" ] || fail "the list must say how long is left: $list"
[ "$(row_field "$list" abandoned url)" = "about:blank" ] \
  || fail "the list must report where the session is: $list"

# Phase 2: driving a second session keeps it alive past the deadline while the
# first is closed for going quiet.
reply="$(cmd launch busy "\"profile_dir\":\"$work/busy\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: second launch failed: $reply"; exit 1; }
deadline=$(( $(date +%s) + (idle_ms / 1000) + 6 ))
while [ "$(date +%s)" -lt "$deadline" ]; do
  cmd get_url busy >/dev/null
  sleep 1
done

list="$(cmd sessions busy)"
echo "$list" | grep -q '"count":1' || fail "exactly the driven session should remain: $list"
[ "$(row_field "$list" busy state)" = "launched" ] || fail "the driven session was closed: $list"
[ -z "$(row_field "$list" abandoned state)" ] \
  || fail "an untouched session outlived its deadline: $list"
grep -q "closed idle context abandoned" "$work/serve.log" \
  || fail "the server did not say it closed the idle session: $(cat "$work/serve.log")"

# Phase 3: explicit close still ends a live session, and says whether one existed.
reply="$(cmd close busy)"
echo "$reply" | grep -q '"closed":true' || fail "close did not report closing a live session: $reply"
reply="$(cmd close busy)"
echo "$reply" | grep -q '"closed":false' || fail "closing twice must not claim a second close: $reply"
list="$(cmd sessions busy)"
echo "$list" | grep -q '"count":0' || fail "the fleet should be empty: $list"

if [ "$failed" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: a live session described itself, an abandoned one was closed on its own, a driven one survived, and close still ends a session"

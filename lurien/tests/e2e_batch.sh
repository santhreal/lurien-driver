#!/usr/bin/env bash
# One call, several verbs, on the real engine over two faces.
#
# Three claims:
#
#   1. A batch runs its steps in order against one page and reports each one, so
#      a login is one round trip rather than four.
#   2. A batch stops at the first failure and says how far the page got: which
#      steps ran, which verb failed, and how many were skipped.
#   3. The CLI and the HTTP face run the same batch. A step list written for one
#      works on the other.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_batch.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7483}"
base="http://127.0.0.1:$port"
ctx="batch-$$"
work="$(mktemp -d)"
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
  [ -n "${server_pid:-}" ] && kill "$server_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; failed=1; }

cmd() {
  local command="$1" extra="${2:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$ctx\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  curl -s --max-time 180 -H 'Content-Type: application/json' -d "$body}" "$base/v1/browser/command"
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
cp "$root/captcha/kinds/fixtures/locator_forms.html" "$work/www/index.html"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -fsS "http://127.0.0.1:$fixture_port/index.html" >/dev/null 2>&1 && break
  sleep 0.25
done
url="http://127.0.0.1:$fixture_port/index.html"

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=4000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# Phase 1: a form filled and submitted in one call.
steps="[\"goto url=$url\",\"eval script=document.getElementById('a4').value='stale'\",\"fill selector=label:Email text=me@example.com\",\"click selector=\\\"role:button=Log in\\\"\",\"title\"]"
reply="$(cmd batch "\"args\":{\"steps\":$steps}")"
echo "$reply" | grep -q '"success":true' || fail "the batch did not run: $reply"
echo "$reply" | grep -q '"ran":5' || fail "the batch did not report five steps: $reply"
log="$(cmd execute_js "\"args\":{\"code\":\"JSON.stringify(window.__log.map(r => r.what))\"}")"
case "$log" in
  *a4:focused*) ;;
  *) fail "the fill step did not reach the field: $log" ;;
esac
case "$log" in
  *a1:clicked*) ;;
  *) fail "the click step did not reach the button: $log" ;;
esac
case "$log" in
  *untrusted*) fail "a batched act arrived as an untrusted event: $log" ;;
esac
filled="$(cmd execute_js '"args":{"code":"document.getElementById(\"a4\").value"}')"
case "$filled" in
  *me@example.com*) ;;
  *) fail "the fill step did not set the requested value: $filled" ;;
esac
case "$filled" in
  *stale*) fail "the fill step appended instead of replacing: $filled" ;;
esac

# Phase 2: a batch that fails mid-way says how far it got.
steps='["title","click selector=\"role:button=Nothing Here\"","fill selector=label:Email text=late"]'
reply="$(cmd batch "\"args\":{\"steps\":$steps}")"
echo "$reply" | grep -q '"success":false' || fail "a failing batch reported success: $reply"
echo "$reply" | grep -q 'batch step 2' || fail "the failure does not name the step: $reply"
echo "$reply" | grep -q '1 title' || fail "the failure does not say what ran: $reply"
echo "$reply" | grep -q '1 step(s) not run' || fail "the failure does not say what was skipped: $reply"
before="$(cmd execute_js "\"args\":{\"code\":\"document.getElementById('a4').value\"}")"
case "$before" in
  *late*) fail "a step after the failure still ran: $before" ;;
esac

# Phase 3: a step list a caller could not have written is refused before anything
# runs, so the page is never half-changed by a typo.
reply="$(cmd batch "\"args\":{\"steps\":\"[\\\"title\\\",\\\"clickk selector=#a1\\\"]\"}")"
echo "$reply" | grep -q '"success":false' || fail "an unknown verb in a step was accepted: $reply"
echo "$reply" | grep -q 'clickk' || fail "the unknown verb was not named: $reply"

cmd close >/dev/null

# Phase 4: the CLI runs the same list.
out="$("$serve" batch "goto url=$url" 'click selector="role:button=Log in"' title 2>&1)"
case "$out" in
  *'"ran":3'*) ;;
  *) fail "the CLI did not run the same batch: $out" ;;
esac

if [ "$failed" -ne 0 ]; then
  echo "--- lurien serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: a batch ran five verbs in one call, replaced the field value, stopped at a failing step and said how far it got, and the CLI ran the same list"

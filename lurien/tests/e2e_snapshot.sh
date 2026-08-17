#!/usr/bin/env bash
# The snapshot an agent acts from, end to end, over the HTTP face.
#
# Four claims:
#
#   1. The default snapshot is a role/name/handle list, not page source: it names
#      the page's controls with the roles a person would use and no markup.
#   2. A handle from that snapshot acts. `ref:eN` clicks the node the line
#      described, and the page records a trusted click on that element.
#   3. A handle that no longer means what it meant is refused, not acted on. The
#      fixture replaces a button's text, and the same handle is then rejected with
#      what changed.
#   4. Source is still reachable on request.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_snapshot.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7482}"
base="http://127.0.0.1:$port"
ctx="snapshot-$$"
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

post() {
  curl -s --max-time 120 -H 'Content-Type: application/json' -d "$1" "$base/v1/browser/command"
}

cmd() {
  local command="$1" extra="${2:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$ctx\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  post "$body}"
}

# The reply's `output` field, decoded, so an assertion reads the text a caller
# reads rather than the JSON around it.
output() {
  printf '%s' "$1" > "$work/reply.json"
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("output") or "")' "$work/reply.json"
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

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=4000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }
reply="$(cmd goto "\"url\":\"http://127.0.0.1:$fixture_port/index.html\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: goto failed: $reply"; exit 1; }

# Phase 1: the default representation.
reply="$(cmd dom_snapshot)"
snap="$(output "$reply")"
printf '%s\n' "$snap" > "$work/snapshot.txt"
case "$snap" in
  *"<button"*|*"<html"*) fail "the default snapshot is page source: $(head -3 "$work/snapshot.txt")" ;;
esac
for expected in 'button "Log in"' 'textbox "Email"' '[ref=e'; do
  case "$snap" in
    *"$expected"*) ;;
    *) fail "the snapshot does not name $expected" ;;
  esac
done

# Phase 2: a handle acts.
handle="$(python3 - "$work/snapshot.txt" <<'PY'
import re, sys
for line in open(sys.argv[1]):
    if 'button "Log in"' in line:
        found = re.search(r"\[ref=(e\d+)\]", line)
        if found:
            print(found.group(1))
            break
PY
)"
if [ -z "$handle" ]; then
  fail "no handle for the login button"
else
  reply="$(cmd dom_click "\"args\":{\"ref\":\"ref:$handle\"}")"
  echo "$reply" | grep -q '"success":true' || fail "the handle $handle did not act: $reply"
  log="$(output "$(cmd execute_js "\"args\":{\"code\":\"JSON.stringify(window.__log.map(r => r.what))\"}")")"
  case "$log" in
    *a1:clicked*) ;;
    *) fail "the handle clicked something else: $log" ;;
  esac
  case "$log" in
    *untrusted*) fail "the handle's click was not a trusted event: $log" ;;
  esac
fi

# Phase 3: a handle whose node changed is refused rather than acted on.
if [ -n "$handle" ]; then
  cmd execute_js "\"args\":{\"code\":\"document.getElementById('a1').textContent = 'Sign out'; 'renamed'\"}" >/dev/null
  reply="$(cmd dom_click "\"args\":{\"ref\":\"ref:$handle\"}")"
  echo "$reply" | grep -q '"success":false' || fail "a stale handle still acted: $reply"
  echo "$reply" | grep -q 'the page changed under the handle' || fail "the stale handle was not explained: $reply"
  echo "$reply" | grep -q 'fresh snapshot' || fail "the refusal does not say what to do: $reply"
fi

# Phase 4: source on request.
source_out="$(output "$(cmd dom_get_source)")"
case "$source_out" in
  *"<button"*) ;;
  *) fail "source is not reachable: ${source_out:0:200}" ;;
esac

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- lurien serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
nodes="$(grep -c '\[ref=e' "$work/snapshot.txt")"
echo "PASS: the snapshot named $nodes addressable nodes, a handle clicked the button it described, a stale handle was refused, and source is still on request"

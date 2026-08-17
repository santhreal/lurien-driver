#!/usr/bin/env bash
# The file chooser a page opens itself, on the real engine.
#
# Four claims:
#
#   1. A page that hides its file input and opens the chooser from a click handler
#      can still be given files: the native dialog never opens, so the session does
#      not stall waiting for someone to answer it.
#   2. The page's own listeners still run on the intercepted click, and its change
#      handler sees the real files with their real sizes.
#   3. Files that do not exist are refused before the page is touched.
#   4. Pressing something that opens no chooser is refused with what to do instead,
#      rather than hanging until the session times out.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_chooser.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7486}"
base="http://127.0.0.1:$port"
ctx="chooser-$$"
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
cp "$root/captcha/kinds/fixtures/file_chooser.html" "$work/www/index.html"
printf 'resume bytes' >"$work/resume.txt"
printf 'cover letter bytes here' >"$work/cover.txt"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -s --max-time 1 "http://127.0.0.1:$fixture_port/index.html" -o /dev/null && break
  sleep 0.2
done
url="http://127.0.0.1:$fixture_port/index.html"

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=5000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"$url\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# Phase 1: a file that does not exist is refused, and the page is untouched.
reply="$(cmd choose_files '"args":{"trigger":"role:button=Attach a resume","files":["/tmp/lurien-no-such-file.txt"]}')"
echo "$reply" | grep -q '"success":false' || fail "a missing file was accepted: $reply"
echo "$reply" | grep -q 'is not a file' || fail "the refusal does not say what is wrong: $reply"
seen="$(cmd execute_js '"args":{"code":"document.getElementById(\"result\").textContent"}')"
case "$seen" in
  *"nothing attached"*) : ;;
  *) fail "the page was changed by a refused call: $seen" ;;
esac

# Phase 2: pressing something that opens no chooser is refused, not waited out.
reply="$(cmd choose_files "\"args\":{\"trigger\":\"#result\",\"files\":[\"$work/resume.txt\"],\"timeout_ms\":2000}")"
echo "$reply" | grep -q '"success":false' || fail "a trigger that opens nothing reported success: $reply"
echo "$reply" | grep -q 'no file chooser opened' || fail "the refusal does not name the problem: $reply"
echo "$reply" | grep -q 'upload' || fail "the refusal does not say what to do instead: $reply"

# Phase 3: the real thing, two files through a chooser the page opened.
reply="$(cmd choose_files "\"args\":{\"trigger\":\"role:button=Attach a resume\",\"files\":[\"$work/resume.txt\",\"$work/cover.txt\"]}")"
echo "$reply" | grep -q '"success":true' || fail "the chooser was not answered: $reply"
echo "$reply" | grep -q 'resume' || fail "the reply does not name the input it answered: $reply"

seen="$(cmd execute_js '"args":{"code":"document.getElementById(\"result\").textContent"}')"
case "$seen" in
  *"resume.txt:12"*) : ;;
  *) fail "the page did not see the real file: $seen" ;;
esac
case "$seen" in
  *"cover.txt:23"*) : ;;
  *) fail "the second file did not arrive: $seen" ;;
esac

# The page's own click listener on the input must still have run: cancelling the
# default action must not cost the page its handlers.
clicks="$(cmd execute_js '"args":{"code":"document.getElementById(\"clicks\").textContent"}')"
case "$clicks" in
  *"clicks seen: 1"*) : ;;
  *) fail "the page's own click listener did not run: $clicks" ;;
esac

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: a script-opened chooser took two real files, the page's own handlers ran, and a bad file and a dead trigger were both refused"

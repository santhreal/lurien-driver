#!/usr/bin/env bash
# The wall clock a page reads, on the real engine.
#
# Six claims:
#
#   1. A page navigated after `clock-set` reads the session's time in its own
#      first script, before anything a driver could inject afterwards.
#   2. A frame reads the same clock as its parent, in whatever process it landed
#      in, and at parse time too.
#   3. The shifted clock is indistinguishable by identity: `Date.prototype` is
#      the native one, `Date.name` is "Date", and the source of `Date.now` and of
#      `Date` itself read as native code through `Function.prototype.toString`.
#   4. `clock-tick` moves a loaded page's clock by exactly the interval, with no
#      reload, and `clock` reports the same time the page reads.
#   5. `clock-restore` gives the host clock back to the loaded page.
#   6. What is not the clock stays untouched: a date built from parts is
#      arithmetic, monotonic time still counts from navigation, and a time that
#      is not a time is refused with the shape that works.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_clock.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7489}"
base="http://127.0.0.1:$port"
ctx="clock-$$"
work="$(mktemp -d)"
failed=0

# The instant every claim is measured against: 2033-05-18T03:33:20Z, far enough
# from now that no rounding can make a host clock look like it.
target_ms=2000000000000
target_year=2033
tick_ms=86400000

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
  local command="$1" extra="${2:-}" context="${3:-$ctx}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$context\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  curl -s --max-time 180 -H 'Content-Type: application/json' -d "$body}" "$base/v1/browser/command"
}

# One field out of a reply's JSON output, which verbs return as a string.
field() {
  printf '%s' "$1" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
out = reply.get("output", "")
try:
    data = json.loads(out)
except Exception:
    print("")
    sys.exit(0)
for key in sys.argv[1:]:
    if isinstance(data, list):
        data = data[int(key)]
    else:
        data = data.get(key, "")
print(data if data is not None else "")
' "${@:2}"
}

# One field out of what the page reports, which is JSON inside the verb's JSON.
read_field() {
  local reply="$1"
  shift
  field "$reply" | python3 -c '
import json, sys
data = json.loads(sys.stdin.read() or "{}")
for key in sys.argv[1:]:
    data = data.get(key)
    if data is None:
        break
print("" if data is None else data)
' "$@"
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
cp "$root/captcha/kinds/fixtures/clock_page.html" "$work/www/index.html"
cp "$root/captcha/kinds/fixtures/clock_frame.html" "$work/www/clock_frame.html"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -s --max-time 1 "http://127.0.0.1:$fixture_port/index.html" -o /dev/null && break
  sleep 0.25
done
url="http://127.0.0.1:$fixture_port/index.html"

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=5000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# A fresh session reads the host clock and says so.
reply="$(cmd clock)"
[ "$(field "$reply" source)" = "host" ] || fail "a fresh session claims a clock of its own: $reply"
[ "$(field "$reply" shift_ms)" = "0" ] || fail "a fresh session is already shifted: $reply"

# Set the clock, then navigate: the page must read it from its first script.
reply="$(cmd clock_set "\"args\":{\"time\":\"$target_ms\"}")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: clock-set failed: $reply"; exit 1; }
[ "$(field "$reply" source)" = "session" ] || fail "a set clock is not reported as the session's: $reply"

cmd goto "\"args\":{\"url\":\"$url\"}" >/dev/null

# Claim 1: parse time, not load time.
reply="$(cmd execute_js '"args":{"code":"window.lurienRead()"}')"
parse_now="$(read_field "$reply" parse now)"
parse_year="$(read_field "$reply" parse year)"
[ -n "$parse_now" ] || { echo "FAIL: the page reported nothing: $reply"; exit 1; }
[ "$parse_year" = "$target_year" ] \
  || fail "the page's first script read year $parse_year, the session serves $target_year"
python3 -c "import sys; sys.exit(0 if abs($parse_now - $target_ms) < 60000 else 1)" \
  || fail "the page's first script read $parse_now, the session serves $target_ms"

# Claim 3: identity is native.
[ "$(read_field "$reply" proto)" = "True" ] || fail "Date.prototype is not the native one: $reply"
[ "$(read_field "$reply" name)" = "Date" ] || fail "Date.name is not Date: $reply"
[ "$(read_field "$reply" tag)" = "[object Date]" ] || fail "a date is not tagged as one: $reply"
for key in source ctorSource; do
  src="$(read_field "$reply" "$key")"
  case "$src" in
    *"[native code]"*) : ;;
    *) fail "$key reads as script, not native code: $src" ;;
  esac
done

# Claim 6: what is not the clock is not moved.
[ "$(read_field "$reply" fixed)" = "1577934245000" ] \
  || fail "a date built from parts moved with the clock: $reply"
[ "$(read_field "$reply" monotonic)" = "True" ] \
  || fail "monotonic time followed the wall clock: $reply"

# Claim 2: the frame reads the same clock, at parse time.
frames="$(cmd dom_frames)"
echo "$frames" | grep -q 'clock_frame' || fail "the fixture frame is not there: $frames"
reply="$(cmd execute_js '"args":{"code":"window.lurienRead()","frame":"clock_frame.html"}')"
frame_now="$(read_field "$reply" parse now)"
frame_year="$(read_field "$reply" parse year)"
if [ -z "$frame_now" ]; then
  fail "the frame reported nothing: $reply"
else
  [ "$frame_year" = "$target_year" ] \
    || fail "the frame's first script read year $frame_year, its parent reads $target_year"
fi

# Claim 4: a tick moves the loaded page by exactly the interval.
before="$(read_field "$(cmd execute_js '"args":{"code":"window.lurienRead()"}')" now)"
reply="$(cmd clock_tick "\"args\":{\"ms\":$tick_ms}")"
echo "$reply" | grep -q '"success":true' || fail "clock-tick failed: $reply"
after="$(read_field "$(cmd execute_js '"args":{"code":"window.lurienRead()"}')" now)"
python3 -c "
import sys
moved = $after - $before
sys.exit(0 if abs(moved - $tick_ms) < 60000 else 1)" \
  || fail "a tick of $tick_ms moved the page by $((after - before))"
reported="$(field "$(cmd clock)" epoch_ms)"
python3 -c "import sys; sys.exit(0 if abs($reported - $after) < 60000 else 1)" \
  || fail "the verb reports $reported, the page reads $after"

# Claim 5: restoring gives the host clock back, with the page still loaded.
reply="$(cmd clock_restore)"
[ "$(field "$reply" source)" = "host" ] || fail "a restored clock is still the session's: $reply"
host_now="$(python3 -c 'import time; print(int(time.time()*1000))')"
now="$(read_field "$(cmd execute_js '"args":{"code":"window.lurienRead()"}')" now)"
python3 -c "import sys; sys.exit(0 if abs($now - $host_now) < 60000 else 1)" \
  || fail "after restoring, the page reads $now and the host reads $host_now"

# Claim 6: a time that is not a time names the shape that works.
reply="$(cmd clock_set '"args":{"time":"tomorrow"}')"
echo "$reply" | grep -q '"success":false' || fail "\"tomorrow\" was accepted as a time: $reply"
echo "$reply" | grep -q '2033-05-18T03:33:20Z' || fail "the refusal does not give the shape: $reply"

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- serve log"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: the page and its frame read the session's clock from their first script, a tick moved a loaded page by the day it asked for, restoring gave the host clock back, and a time that is not a time named the shape that works"

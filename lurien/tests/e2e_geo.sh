#!/usr/bin/env bash
# Position and permissions on the real engine.
#
# Six claims:
#
#   1. A page that may read its position gets the persona's own coordinates, not
#      the host's, from the stock navigator.geolocation.
#   2. What the page reads is what the `geolocation` verb reports, to the digit.
#   3. `geolocation-set` moves the session while the page stays loaded: the next
#      fix is the new place, with no reload and no page script patched.
#   4. `geolocation-clear` puts the persona's coordinates back, and the report
#      flips from override to persona.
#   5. A session that was not launched with the permission is refused with
#      PERMISSION_DENIED, and both the verb and the Permissions API say so.
#   6. A coordinate no place has is refused with the range, and a permission
#      change mid-session is refused with the launch argument that works.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_geo.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7488}"
base="http://127.0.0.1:$port"
ctx="geo-$$"
denied_ctx="geo-denied-$$"
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

# Ask the page for a fix and return what it got, as JSON. Polls because a fix is
# asynchronous; the page parks it in a global rather than resolving a promise.
page_fix() {
  local context="${1:-$ctx}"
  cmd execute_js '"args":{"code":"window.lurienAsk()"}' "$context" >/dev/null
  local i reply read
  for i in $(seq 1 60); do
    reply="$(cmd execute_js '"args":{"code":"window.lurienRead()"}' "$context")"
    read="$(field "$reply")"
    case "$read" in
      *'"fix":{'*|*'"err":{'*) printf '%s' "$read"; return 0 ;;
    esac
    sleep 0.25
  done
  printf '%s' '{"fix":null,"err":null}'
}

# One number, rounded so a float printed two ways matches, and printed without a
# decimal tail when it is integral so an error code reads as 1.
number() {
  printf '%s' "${1:-}" | python3 -c '
import sys
raw = sys.stdin.read().strip()
if not raw:
    print("")
else:
    value = round(float(raw), 4)
    print(int(value) if value == int(value) else value)
'
}

# One number out of the page's fix.
fix_number() {
  printf '%s' "$1" | python3 -c '
import json, sys
data = json.load(sys.stdin)
part = data.get(sys.argv[1]) or {}
value = part.get(sys.argv[2])
print("" if value is None else value)
' "$2" "$3" | { read -r raw; number "$raw"; }
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
cp "$root/captcha/kinds/fixtures/geo_page.html" "$work/www/index.html"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -s -o /dev/null "http://127.0.0.1:$fixture_port/index.html" && break
  sleep 0.25
done
url="http://127.0.0.1:$fixture_port/index.html"

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=5000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

# A session that may read its position.
reply="$(cmd launch "\"profile_dir\":\"$work/allowed\",\"url\":\"about:blank\",\"args\":{\"allow\":\"geolocation\"}")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

reply="$(cmd permissions)"
[ "$(field "$reply" geolocation)" = "allow" ] \
  || fail "the session does not report the permission it launched with: $reply"
[ "$(field "$reply" camera)" = "deny" ] \
  || fail "a permission nobody asked for is not denied: $reply"

reply="$(cmd geolocation)"
persona_lat="$(number "$(field "$reply" latitude)")"
persona_lon="$(number "$(field "$reply" longitude)")"
[ "$(field "$reply" source)" = "persona" ] || fail "a fresh session claims an override: $reply"
[ "$(field "$reply" permission)" = "allow" ] || fail "the report disagrees with the policy: $reply"

# Claim 1 and 2: the page reads the persona's coordinates, digit for digit.
cmd goto "\"args\":{\"url\":\"$url\"}" >/dev/null
fix="$(page_fix)"
lat="$(fix_number "$fix" fix latitude)"
lon="$(fix_number "$fix" fix longitude)"
[ -n "$lat" ] || fail "the page got no fix at all: $fix"
[ "$lat" = "$persona_lat" ] && [ "$lon" = "$persona_lon" ] \
  || fail "page read $lat,$lon; the session serves $persona_lat,$persona_lon"

perm="$(field "$(cmd execute_js '"args":{"code":"window.lurienPermission"}')")"
[ "$perm" = "granted" ] || fail "the Permissions API says $perm, the session says allow"

# Claim 3: the position moves under a loaded page.
reply="$(cmd geolocation_set '"args":{"latitude":-33.8688,"longitude":151.2093,"accuracy_m":25}')"
[ "$(field "$reply" source)" = "override" ] || fail "a set position is not reported as an override: $reply"
fix="$(page_fix)"
lat="$(fix_number "$fix" fix latitude)"
lon="$(fix_number "$fix" fix longitude)"
acc="$(fix_number "$fix" fix accuracy)"
[ "$lat" = "-33.8688" ] && [ "$lon" = "151.2093" ] \
  || fail "the page did not move: read $lat,$lon after a set to -33.8688,151.2093"
[ "$acc" = "25" ] || fail "the page reports accuracy $acc, the session served 25"

# Claim 4: clearing puts the persona back.
reply="$(cmd geolocation_clear)"
[ "$(field "$reply" source)" = "persona" ] || fail "a cleared override is still an override: $reply"
fix="$(page_fix)"
lat="$(fix_number "$fix" fix latitude)"
[ "$lat" = "$persona_lat" ] || fail "after clearing, the page reads $lat, persona is $persona_lat"

# Claim 6: refusals name the range and the launch argument.
reply="$(cmd geolocation_set '"args":{"latitude":95,"longitude":0}')"
echo "$reply" | grep -q '"success":false' || fail "latitude 95 was accepted: $reply"
echo "$reply" | grep -q -- '-90 to 90' || fail "the refusal does not give the range: $reply"
reply="$(cmd permissions '"args":{"allow":"camera"}')"
echo "$reply" | grep -q '"success":false' || fail "a mid-session permission change was accepted: $reply"
echo "$reply" | grep -q -- '--allow' || fail "the refusal does not name the launch argument: $reply"

cmd close >/dev/null

# Claim 5: a session that was not granted the permission is refused, not stalled.
reply="$(cmd launch "\"profile_dir\":\"$work/denied\",\"url\":\"$url\"" "$denied_ctx")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: second launch failed: $reply"; exit 1; }
reply="$(cmd permissions "" "$denied_ctx")"
[ "$(field "$reply" geolocation)" = "deny" ] || fail "geolocation is granted by default: $reply"
fix="$(page_fix "$denied_ctx")"
code="$(fix_number "$fix" err code)"
[ "$code" = "1" ] || fail "a denied session did not report PERMISSION_DENIED: $fix"
perm="$(field "$(cmd execute_js '"args":{"code":"window.lurienPermission"}' "$denied_ctx")")"
[ "$perm" = "denied" ] || fail "the Permissions API says $perm in a denied session"
cmd close "" "$denied_ctx" >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: the page read the persona's coordinates, followed a live move to Sydney and back, a denied session was refused with code 1, and both impossible asks named the fix"

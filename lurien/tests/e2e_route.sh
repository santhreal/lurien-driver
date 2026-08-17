#!/usr/bin/env bash
# What routes do to real requests, on the real engine.
#
# Eight claims:
#
#   1. A fulfil route answers from the browser: the page reads the status, the
#      content type, and the body the route named, and the server never saw the
#      request.
#   2. An abort route is a network error to the page, not an empty response.
#   3. A continue route edits request headers: the header it sets arrives at the
#      server and the header it removes does not, with the request still served
#      by the server.
#   4. The most recently added route wins, so a caller narrows behaviour by
#      adding a route rather than by withdrawing one.
#   5. `route` reports the table in match order with a count per route.
#   6. `route-clear` gives the network back: the same request reaches the server.
#   7. A route that cannot work is refused with the shape that works, and the
#      table is left as it was.
#   8. A legacy header command lands on a real route, so an old client changes
#      the request the server receives instead of a page global nothing reads.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_route.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7490}"
base="http://127.0.0.1:$port"
ctx="route-$$"
work="$(mktemp -d)"
failed=0

if [ -z "$engine" ] || [ ! -x "$engine" ]; then
  echo "SKIP: set LURIEN_BIN to the engine binary"
  exit 0
fi
if [ ! -x "$serve" ]; then
  echo "SKIP: build the driver first (cargo build -p lurien-driver)"
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

# One field out of what the page reported, which is JSON inside the verb's JSON.
read_field() {
  local reply="$1"
  shift
  printf '%s' "$reply" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
data = reply.get("output", "")
if isinstance(data, str):
    try:
        data = json.loads(data or "{}")
    except Exception:
        data = {}
for key in sys.argv[1:]:
    if not isinstance(data, dict):
        data = None
        break
    data = data.get(key)
    if data is None:
        break
print("" if data is None else data)
' "$@"
}

# The fixture server: it counts what reaches it and echoes what it was sent, so a
# request a route answered and a request a route edited are told apart from the
# server's side as well as the page's.
cat >"$work/server.py" <<'PY'
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PAGE = open(sys.argv[2], "rb").read()
HITS = {}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def send_json(self, payload):
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-Served-By", "server")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/hits":
            self.send_json(HITS)
            return
        HITS[path] = HITS.get(path, 0) + 1
        if path == "/echo":
            self.send_json({
                "headers": {k.lower(): v for k, v in self.headers.items()},
                "hits": HITS[path],
            })
            return
        if path == "/api/data":
            self.send_json({"origin": "server", "hits": HITS[path]})
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
python3 "$work/server.py" "$fixture_port" "$root/captcha/kinds/fixtures/route_page.html" >/dev/null 2>&1 &
server_pid=$!
origin="http://127.0.0.1:$fixture_port"
for _ in $(seq 1 40); do
  curl -s --max-time 2 "$origin/hits" -o /dev/null && break
  sleep 0.25
done
url="$origin/index.html"
api="$origin/api/data"
echo_url="$origin/echo"

hits() { curl -s --max-time 5 "$origin/hits" | python3 -c "
import json, sys
print(json.load(sys.stdin).get('$1', 0))"; }

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=15000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# A fresh session routes nothing.
reply="$(cmd route)"
[ "$(field "$reply" count)" = "0" ] || fail "a fresh session already has routes: $reply"

cmd goto "\"args\":{\"url\":\"$url\"}" >/dev/null

# Claim 1: a fulfil route answers, and the server never sees the request.
reply="$(cmd route_fulfil "\"args\":{\"pattern\":\"*/api/*\",\"status\":201,\"status_text\":\"Created\",\"headers\":\"{\\\"Content-Type\\\":\\\"application/json\\\"}\",\"body\":\"{\\\"origin\\\":\\\"route\\\"}\"}")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: route-fulfil failed: $reply"; exit 1; }
before="$(hits /api/data)"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$api')\"}")"
[ "$(read_field "$reply" status)" = "201" ] || fail "a fulfilled request did not carry the status: $reply"
[ "$(read_field "$reply" statusText)" = "Created" ] || fail "a fulfilled request did not carry the reason: $reply"
[ "$(read_field "$reply" body)" = '{"origin":"route"}' ] || fail "a fulfilled request did not carry the body: $reply"
case "$(read_field "$reply" type)" in
  application/json*) ;;
  *) fail "a fulfilled request did not carry the content type: $reply" ;;
esac
[ "$(read_field "$reply" trace)" = "" ] || fail "a fulfilled request reached the server: $reply"
[ "$(hits /api/data)" = "$before" ] || fail "the server counted a request a route answered"

# Claim 4: the most recently added route wins.
reply="$(cmd route_fulfil "\"args\":{\"pattern\":\"*/api/*\",\"body\":\"newer\"}")"
echo "$reply" | grep -q '"success":true' || fail "a second route on the same pattern was refused: $reply"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$api')\"}")"
[ "$(read_field "$reply" body)" = "newer" ] \
  || fail "the older route on the same pattern still won: $reply"
case "$(read_field "$reply" type)" in
  text/plain*) ;;
  *) fail "a body served with no content type did not get the default: $reply" ;;
esac

# Claim 5: the table reads in match order, with a count per route.
reply="$(cmd route)"
[ "$(field "$reply" count)" = "2" ] || fail "the table is not two routes: $reply"
[ "$(field "$reply" routes 0 body_bytes)" = "5" ] || fail "the newest route is not first: $reply"
[ "$(field "$reply" routes 0 hits)" = "1" ] || fail "the newest route reports no hit: $reply"
[ "$(field "$reply" routes 1 hits)" = "1" ] || fail "the older route lost its count: $reply"

# Claim 3: a continue route edits the request the server receives.
reply="$(cmd route_continue "\"args\":{\"pattern\":\"*/echo*\",\"headers\":\"{\\\"X-Trace\\\":\\\"routed\\\"}\",\"remove\":\"X-Page\"}")"
echo "$reply" | grep -q '"success":true' || fail "route-continue failed: $reply"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$echo_url', {'X-Page': 'from-page'})\"}")"
body="$(read_field "$reply" body)"
[ "$(read_field "$reply" trace)" = "server" ] || fail "an edited request did not reach the server: $reply"
printf '%s' "$body" | grep -q '"x-trace": *"routed"' || fail "the header the route set did not arrive: $body"
printf '%s' "$body" | grep -q '"x-page"' && fail "the header the route removed still arrived: $body"

# Claim 2: an abort is a network error.
reply="$(cmd route_abort "\"args\":{\"pattern\":\"*/echo*\"}")"
echo "$reply" | grep -q '"success":true' || fail "route-abort failed: $reply"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$echo_url')\"}")"
[ "$(read_field "$reply" ok)" = "False" ] || fail "an aborted request still resolved: $reply"
read_field "$reply" error | grep -qi 'networkerror\|failed' || fail "an abort is not reported as a network error: $reply"

# Claim 7: a route that cannot work is refused, and the table is unchanged.
before="$(field "$(cmd route)" count)"
reply="$(cmd route_fulfil '"args":{"pattern":"*","status":999}')"
echo "$reply" | grep -q '"success":false' || fail "status 999 was accepted: $reply"
echo "$reply" | grep -q '100 to 599' || fail "the refusal does not name the range: $reply"
reply="$(cmd route_continue '"args":{"pattern":"*"}')"
echo "$reply" | grep -q '"success":false' || fail "a route that edits nothing was accepted: $reply"
echo "$reply" | grep -q 'route-abort' || fail "the refusal does not name the alternative: $reply"
[ "$(field "$(cmd route)" count)" = "$before" ] || fail "a refused route changed the table"

# Claim 8: a legacy header command is a real route now.
cmd route_clear >/dev/null
reply="$(cmd dom_set_header '"args":{"name":"X-Legacy","value":"yes"}')"
echo "$reply" | grep -q '"success":true' || fail "dom_set_header failed: $reply"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$echo_url')\"}")"
printf '%s' "$(read_field "$reply" body)" | grep -q '"x-legacy": *"yes"' \
  || fail "a legacy header command did not change the request: $reply"

# Claim 6: clearing gives the network back.
reply="$(cmd route_clear)"
[ "$(field "$reply" count)" = "0" ] || fail "the table is not empty after route-clear: $reply"
before="$(hits /api/data)"
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$api')\"}")"
[ "$(read_field "$reply" trace)" = "server" ] || fail "after route-clear the request did not reach the server: $reply"
printf '%s' "$(read_field "$reply" body)" | grep -q '"origin": *"server"' \
  || fail "after route-clear the page still read a route's body: $reply"
[ "$(hits /api/data)" -gt "$before" ] || fail "after route-clear the server counted nothing"

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "serve log:"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: a fulfil route answered without the server, an abort was a network error, a continue route changed the request the server received, the newest route won, the table reported its counts, a refusal named the fix, a legacy header command routed, and clearing gave the network back"

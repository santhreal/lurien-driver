#!/usr/bin/env bash
# The HAR a session exports, from real captured traffic.
#
# Four claims:
#
#   1. `har` with a path writes a file that parses as HAR 1.2, with a creator and
#      one entry per request the session made.
#   2. A credential is not in it: not the Authorization header a route added, not
#      the token in a query, not a cookie value. The rows themselves are still
#      there, with the header names and the redaction markers.
#   3. What a reader needs survives: method, status, an ISO start time, and the
#      response mime type of a request that really happened.
#   4. Without a path the log comes back inline, so a client that cannot read the
#      session's filesystem still gets the export.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_har.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7491}"
base="http://127.0.0.1:$port"
ctx="har-$$"
work="$(mktemp -d)"
failed=0
secret="live-secret-do-not-export"

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

cat >"$work/server.py" <<'PY'
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PAGE = b"""<!doctype html><html><head><meta charset="utf-8"><title>har</title></head>
<body><script>window.lurienFetch = async (url) => {
  try { const r = await fetch(url, { cache: "no-store" }); return { status: r.status }; }
  catch (e) { return { error: String(e) }; }
};</script></body></html>"""


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/data":
            body = b'{"ok":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Set-Cookie", "sid=cookie-secret-value; Path=/")
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
python3 "$work/server.py" "$fixture_port" >/dev/null 2>&1 &
server_pid=$!
origin="http://127.0.0.1:$fixture_port"
for _ in $(seq 1 40); do
  curl -s --max-time 2 "$origin/index.html" -o /dev/null && break
  sleep 0.25
done

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=15000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# Traffic worth exporting: a page, then a request carrying a credential in a
# header a route added and another in the query.
reply="$(cmd route_continue "\"args\":{\"pattern\":\"*/data*\",\"headers\":\"{\\\"Authorization\\\":\\\"Bearer $secret\\\"}\"}")"
echo "$reply" | grep -q '"success":true' || fail "route-continue failed: $reply"
cmd goto "\"args\":{\"url\":\"$origin/index.html\"}" >/dev/null
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienFetch('$origin/data?access_token=$secret&page=2')\"}")"
echo "$reply" | grep -q '"status":200' || fail "the fixture request did not succeed: $reply"

# Claim 1: a file that parses, with entries in it.
reply="$(cmd har "\"args\":{\"path\":\"$work/session.har\"}")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: har failed: $reply"; exit 1; }
[ -s "$work/session.har" ] || fail "har wrote no file: $reply"
python3 - "$work/session.har" "$secret" <<'PY' || failed=1
import json, sys

har = json.load(open(sys.argv[1]))
secret = sys.argv[2]
log = har["log"]
ok = True


def bad(message):
    global ok
    print(f"FAIL: {message}")
    ok = False


if log["version"] != "1.2":
    bad(f"har version is {log['version']!r}")
if log["creator"]["name"] != "lurien":
    bad(f"creator is {log['creator']!r}")
entries = log["entries"]
if not entries:
    bad("the har has no entries")

# Claim 2: no credential anywhere in the document, and the rows are still rows.
raw = json.dumps(har)
for leak in (secret, "cookie-secret-value"):
    if leak in raw:
        bad(f"the har carries {leak!r}")
if "***redacted***" not in raw:
    bad("nothing was redacted, which means nothing was recognised")

data = [e for e in entries if "/data" in e["request"]["url"]]
if not data:
    bad(f"no entry for the fixture request: {[e['request']['url'] for e in entries]}")
else:
    entry = data[0]
    if "access_token=<redacted>" not in entry["request"]["url"]:
        bad(f"the query token is not redacted: {entry['request']['url']}")
    if "page=2" not in entry["request"]["url"]:
        bad(f"the harmless query value was dropped: {entry['request']['url']}")
    names = [h["name"].lower() for h in entry["request"]["headers"]]
    if "authorization" not in names:
        bad(f"the header the route added is not in the har: {names}")
    auth = next(h["value"] for h in entry["request"]["headers"] if h["name"].lower() == "authorization")
    if auth != "Bearer ***redacted***":
        bad(f"the authorization header reads {auth!r}")
    # Claim 3: what a reader needs.
    if entry["request"]["method"] != "GET":
        bad(f"method is {entry['request']['method']!r}")
    if entry["response"]["status"] != 200:
        bad(f"status is {entry['response']['status']!r}")
    if not entry["startedDateTime"].endswith("Z") or "T" not in entry["startedDateTime"]:
        bad(f"startedDateTime is {entry['startedDateTime']!r}")
    if "json" not in entry["response"]["content"]["mimeType"]:
        bad(f"mime type is {entry['response']['content']['mimeType']!r}")

sys.exit(0 if ok else 1)
PY

# Claim 4: without a path the log comes back inline.
reply="$(cmd har)"
[ "$(field "$reply" log version)" = "1.2" ] || fail "an inline export carries no log: $reply"
printf '%s' "$reply" | grep -q "$secret" && fail "an inline export carries the credential"

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "serve log:"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: the export parses as HAR 1.2, carries the rows with header names and redaction markers, keeps method, status, start time and mime type, and carries no credential from a header, a query or a cookie"

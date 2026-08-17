#!/usr/bin/env bash
# Downloads on the real engine: a file the page hands over must arrive as bytes.
#
# Four claims:
#
#   1. A link the browser saves lands in this session's download directory, with
#      no prompt and no window left open.
#   2. `download-wait` returns only when the bytes are on disk, and names the
#      file, its size and its path.
#   3. `download-save` copies the file where the caller asked, byte for byte.
#   4. A file that never arrives is refused with what the page did start, rather
#      than hanging or reporting an empty list as success.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_downloads.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7485}"
base="http://127.0.0.1:$port"
ctx="downloads-$$"
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

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www" "$work/downloads"
cp "$root/captcha/kinds/fixtures/download_page.html" "$work/www/index.html"
# A payload big enough that a truncated copy is obvious, and not text, so the
# browser must save it rather than render it.
python3 -c "
import pathlib
pathlib.Path('$work/www/payload.bin').write_bytes(bytes(range(256)) * 400)
"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -s --max-time 1 "http://127.0.0.1:$fixture_port/index.html" -o /dev/null && break
  sleep 0.2
done
url="http://127.0.0.1:$fixture_port/index.html"
expected_size="$(python3 -c "import os; print(os.path.getsize('$work/www/payload.bin'))")"

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=5000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\",\"args\":{\"download_dir\":\"$work/downloads\"}")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }

# Phase 1: nothing has been downloaded, and the session says where files will go.
reply="$(cmd downloads)"
echo "$reply" | grep -q '"count":0' || fail "a fresh session claims a download: $reply"
[ "$(field "$reply" dir)" = "$work/downloads" ] \
  || fail "the session does not report its own download directory: $reply"

# Phase 2: a file that never arrives is refused with what the page started.
reply="$(cmd download_wait "\"args\":{\"name\":\"nothing.zip\",\"timeout_ms\":1500}")"
echo "$reply" | grep -q '"success":false' || fail "waiting for nothing reported success: $reply"
echo "$reply" | grep -q 'nothing.zip' || fail "the refusal does not name the file: $reply"
echo "$reply" | grep -q 'seen so far: nothing' || fail "the refusal does not say what it saw: $reply"

# Phase 3: the link the browser saves.
cmd goto "\"args\":{\"url\":\"$url\"}" >/dev/null
cmd dom_click '"args":{"selector":"#direct"}' >/dev/null
reply="$(cmd download_wait '"args":{"name":"payload","timeout_ms":30000}')"
echo "$reply" | grep -q '"success":true' || fail "the download never finished: $reply"
size="$(field "$reply" size_bytes)"
[ "$size" = "$expected_size" ] || fail "downloaded $size bytes, served $expected_size: $reply"
path="$(field "$reply" path)"
case "$path" in
  "$work/downloads"/*) : ;;
  *) fail "the file landed outside the session directory: $path" ;;
esac
[ "$(field "$reply" on_disk)" = "True" ] || fail "the reply claims no bytes on disk: $reply"

# Phase 4: saving it out, byte for byte.
reply="$(cmd download_save "\"args\":{\"path\":\"$work/out/copy.bin\",\"name\":\"payload\",\"timeout_ms\":30000}")"
echo "$reply" | grep -q '"success":true' || fail "save failed: $reply"
[ -f "$work/out/copy.bin" ] || fail "save reported success with no file: $reply"
if ! cmp -s "$work/www/payload.bin" "$work/out/copy.bin"; then
  fail "the saved copy differs from what the server served"
fi

# Phase 5: a blob a script builds is a download too, and the list carries both.
cmd dom_click '"args":{"selector":"#generated"}' >/dev/null
reply="$(cmd download_wait '"args":{"name":"report.csv","timeout_ms":30000}')"
echo "$reply" | grep -q '"success":true' || fail "the generated file never arrived: $reply"
reply="$(cmd downloads)"
echo "$reply" | grep -q 'payload.bin' || fail "the list lost the first download: $reply"
echo "$reply" | grep -q 'report.csv' || fail "the list lost the generated download: $reply"

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: a saved link and a generated blob both landed in the session directory, the wait returned real bytes, the copy matched, and a file that never came was refused"

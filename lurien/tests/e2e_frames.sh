#!/usr/bin/env bash
# Frame handles against a real cross-origin frame.
#
# Six claims:
#
#   1. `frames` names every context, main document first, and each name is a
#      handle a caller can hold.
#   2. A handle addresses the frame it named: `eval` in it reads that document,
#      not the parent's.
#   3. The handle survives a navigation of that frame. The url in the table moves
#      and the handle does not, which is the whole point: a caller that stored
#      `url:` or an index would now be addressing a different document or none.
#   4. Acting through a handle works after that navigation: type-in and click-in
#      reach the reloaded document.
#   5. A frame that attaches later gets its own handle, and no existing handle
#      changes meaning.
#   6. A handle whose frame is gone is refused, and the refusal says what it was
#      and what to run. It is never resolved to another frame.
#
# The child frame is served from a second origin, so it is an out-of-process
# frame: the case where parent JavaScript cannot reach it at all.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_frames.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7492}"
base="http://127.0.0.1:$port"
ctx="frames-$$"
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
  [ -n "${parent_pid:-}" ] && kill "$parent_pid" 2>/dev/null
  [ -n "${child_pid:-}" ] && kill "$child_pid" 2>/dev/null
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

# The handle of the frame whose url contains a substring, out of `frames`.
handle_of() {
  printf '%s' "$1" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
data = json.loads(reply.get("output", "{}") or "{}")
for frame in data.get("frames", []):
    if sys.argv[1] in (frame.get("url") or ""):
        print(frame.get("handle") or "")
        break
' "$2"
}

# One field of what the child document reports about itself.
child_field() {
  printf '%s' "$1" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
data = json.loads(reply.get("output", "{}") or "{}")
if isinstance(data, str):
    data = json.loads(data or "{}")
print(data.get(sys.argv[1], ""))
' "$2"
}

parent_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
child_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
# 127.0.0.1 and localhost are two origins, so the child frame lands in its own
# process the way a real embedded widget does.
child_origin="http://localhost:$child_port"
mkdir -p "$work/parent" "$work/child"
python3 - "$root/captcha/kinds/fixtures/frame_parent.html" "$work/parent/index.html" "$child_origin/frame_child.html" <<'PY'
import sys
src, dest, child = sys.argv[1:4]
open(dest, "w").write(open(src).read().replace("CHILD_URL", child))
PY
cp "$root/captcha/kinds/fixtures/frame_child.html" "$work/child/frame_child.html"
( cd "$work/parent" && exec python3 -m http.server "$parent_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
parent_pid=$!
( cd "$work/child" && exec python3 -m http.server "$child_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
child_pid=$!
url="http://127.0.0.1:$parent_port/index.html"
for _ in $(seq 1 40); do
  curl -s --max-time 2 "$url" -o /dev/null && curl -s --max-time 2 "$child_origin/frame_child.html" -o /dev/null && break
  sleep 0.25
done

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=15000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }
cmd goto "\"args\":{\"url\":\"$url\"}" >/dev/null

# Claim 1: every context is named, main document first.
frames="$(cmd dom_frames)"
[ "$(field "$frames" frames 0 is_main)" = "True" ] || fail "the first frame is not the main document: $frames"
[ "$(field "$frames" frames 0 handle)" = "f1" ] || fail "the main document is not f1: $frames"
child="$(handle_of "$frames" frame_child.html)"
[ -n "$child" ] || { echo "FAIL: the cross-origin child frame has no handle: $frames"; exit 1; }

# Claim 2: the handle reads that document, not the parent's.
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienState()\",\"frame\":\"$child\"}")"
[ "$(child_field "$reply" pass)" = "1" ] || fail "$child did not read the child document: $reply"

# Claim 3: navigate the frame; the handle stays, the url moves.
cmd execute_js '"args":{"code":"window.lurienNavigate(2)"}' >/dev/null
for _ in $(seq 1 40); do
  reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienState()\",\"frame\":\"$child\"}")"
  [ "$(child_field "$reply" pass)" = "2" ] && break
  sleep 0.25
done
[ "$(child_field "$reply" pass)" = "2" ] \
  || fail "after navigating, $child does not read the new document: $reply"
frames="$(cmd dom_frames)"
[ "$(handle_of "$frames" 'pass=2')" = "$child" ] \
  || fail "the handle changed across a navigation: $frames"

# Claim 4: acting through the handle reaches the reloaded document.
cmd dom_type "\"args\":{\"frame\":\"$child\",\"selector\":\"#field\",\"text\":\"typed-after-reload\"}" >/dev/null
cmd dom_click "\"args\":{\"frame\":\"$child\",\"selector\":\"#go\"}" >/dev/null
reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienState()\",\"frame\":\"$child\"}")"
[ "$(child_field "$reply" typed)" = "typed-after-reload" ] \
  || fail "typing through the handle did not reach the frame: $reply"
[ "$(child_field "$reply" clicked)" = "True" ] \
  || fail "clicking through the handle did not reach the frame: $reply"

# Claim 5: a frame that attaches later is named, and nothing renames.
cmd execute_js "\"args\":{\"code\":\"window.lurienAddFrame('$child_origin/frame_child.html?pass=9')\"}" >/dev/null
for _ in $(seq 1 40); do
  frames="$(cmd dom_frames)"
  later="$(handle_of "$frames" 'pass=9')"
  [ -n "$later" ] && break
  sleep 0.25
done
[ -n "$later" ] || fail "the frame that attached later has no handle: $frames"
[ "$later" != "$child" ] || fail "the new frame took the first frame's handle: $frames"
[ "$(handle_of "$frames" 'pass=2')" = "$child" ] || fail "an existing handle moved: $frames"

# Claim 6: a handle whose frame is gone is refused with what it was.
cmd execute_js '"args":{"code":"window.lurienDropFrame()"}' >/dev/null
for _ in $(seq 1 40); do
  reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienState()\",\"frame\":\"$child\"}")"
  echo "$reply" | grep -q '"success":false' && break
  sleep 0.25
done
echo "$reply" | grep -q '"success":false' || fail "a handle for a frame that is gone still resolved: $reply"
echo "$reply" | grep -q "$child is gone" || fail "the refusal does not name the handle: $reply"
echo "$reply" | grep -q 'pass=2' || fail "the refusal does not say what the frame was: $reply"
echo "$reply" | grep -qi 'run frames' || fail "the refusal does not name what to run: $reply"

# A handle this session never minted is refused the same way.
reply="$(cmd execute_js '"args":{"code":"1","frame":"f99"}')"
echo "$reply" | grep -q '"success":false' || fail "an invented handle resolved: $reply"
echo "$reply" | grep -q 'no frame is named f99' || fail "the refusal does not name the handle: $reply"

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "serve log:"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: every context is named, a handle read and acted on its own cross-origin document, it survived a navigation that moved the url, a later frame got its own name, and a handle whose frame is gone was refused with what it was"

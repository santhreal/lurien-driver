#!/usr/bin/env bash
# What a capture covers, end to end, over the HTTP face.
#
# Six claims, all checked against geometry the page reports about itself rather
# than numbers assumed here, so a different window size or device pixel ratio
# cannot turn a correct capture into a failure:
#
#   1. A viewport capture is the size of the viewport.
#   2. A full-page capture is the size of the whole document: taller than the
#      viewport, and as tall as the page says it scrolls.
#   3. A clip captures exactly the rectangle asked for, including one that is far
#      below the fold, with no scrolling.
#   4. A selector captures exactly the element's own box, and a semantic form
#      describes the element as readily as CSS does.
#   5. A frame captures that frame's own document, at the frame's viewport size
#      and, with full_page, at the inner document's larger size.
#   6. Contradictory or impossible areas are refused with what to do instead: two
#      areas at once, a malformed rectangle, and an element with no box.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_shot.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7487}"
base="http://127.0.0.1:$port"
ctx="shot-$$"
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

output() {
  printf '%s' "$1" > "$work/reply.json"
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("output") or "")' "$work/reply.json"
}

error_of() {
  printf '%s' "$1" > "$work/reply.json"
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d.get("error") or d.get("output") or "")' "$work/reply.json"
}

# "png 12345 bytes, 1280x2400" -> "1280x2400"
dims() {
  printf '%s' "$1" | python3 -c 'import re,sys; m=re.search(r"(\d+)x(\d+)", sys.stdin.read()); print(f"{m.group(1)}x{m.group(2)}" if m else "")'
}

# Compare a captured size against a CSS size scaled by the device pixel ratio.
# Rounding at composite time is allowed to move a pixel; ten is not.
close_to() {
  python3 - "$1" "$2" "$3" "$4" <<'PY'
import sys
got, css_w, css_h, dpr = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
if "x" not in got:
    sys.exit(1)
gw, gh = (int(v) for v in got.split("x"))
want_w, want_h = round(css_w * dpr), round(css_h * dpr)
sys.exit(0 if abs(gw - want_w) <= 2 and abs(gh - want_h) <= 2 else 1)
PY
}

# A size proves how much was captured; the colour at the centre proves which
# pixels. Without it a capture whose rectangle landed in the wrong place would
# still be the right size.
cat > "$work/pixel.py" <<'PY'
import struct, sys, zlib

path, want = sys.argv[1], sys.argv[2]
data = open(path, "rb").read()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    print("not a PNG")
    raise SystemExit(2)
pos, idat, ihdr = 8, bytearray(), None
while pos < len(data):
    (length,) = struct.unpack(">I", data[pos : pos + 4])
    kind = data[pos + 4 : pos + 8]
    chunk = data[pos + 8 : pos + 8 + length]
    pos += 12 + length
    if kind == b"IHDR":
        ihdr = struct.unpack(">IIBBBBB", chunk)
    elif kind == b"IDAT":
        idat += chunk
    elif kind == b"IEND":
        break
width, height, depth, colour, _comp, _filt, interlace = ihdr
if depth != 8 or colour not in (2, 6) or interlace:
    print(f"unsupported PNG: depth {depth} colour {colour} interlace {interlace}")
    raise SystemExit(2)
step = 3 if colour == 2 else 4
stride = width * step
raw = zlib.decompress(bytes(idat))
row = bytearray(stride)
prev = bytearray(stride)
at = 0
target = height // 2
for y in range(target + 1):
    filt = raw[at]
    at += 1
    row = bytearray(raw[at : at + stride])
    at += stride
    if filt == 1:
        for i in range(step, stride):
            row[i] = (row[i] + row[i - step]) & 255
    elif filt == 2:
        for i in range(stride):
            row[i] = (row[i] + prev[i]) & 255
    elif filt == 3:
        for i in range(stride):
            left = row[i - step] if i >= step else 0
            row[i] = (row[i] + ((left + prev[i]) >> 1)) & 255
    elif filt == 4:
        for i in range(stride):
            left = row[i - step] if i >= step else 0
            up = prev[i]
            up_left = prev[i - step] if i >= step else 0
            pa, pb, pc = abs(up - up_left), abs(left - up_left), abs(left + up - 2 * up_left)
            best = left if (pa <= pb and pa <= pc) else (up if pb <= pc else up_left)
            row[i] = (row[i] + best) & 255
    prev = row
centre = (width // 2) * step
got = tuple(row[centre : centre + 3])
wanted = tuple(int(want[i : i + 2], 16) for i in (0, 2, 4))
if max(abs(a - b) for a, b in zip(got, wanted)) > 8:
    print(f"centre pixel is {got}, wanted {wanted}")
    raise SystemExit(1)
PY

centre_is() {
  python3 "$work/pixel.py" "$1" "$2"
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
cp "$root/captcha/kinds/fixtures/shot_geometry.html" "$work/www/index.html"
cp "$root/captcha/kinds/fixtures/shot_frame.html" "$work/www/shot_frame.html"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -s --max-time 1 "http://127.0.0.1:$fixture_port/index.html" -o /dev/null && break
  sleep 0.25
done

LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=5000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

reply="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: launch failed: $reply"; exit 1; }
reply="$(cmd goto "\"url\":\"http://127.0.0.1:$fixture_port/index.html\"")"
echo "$reply" | grep -q '"success":true' || { echo "FAIL: goto failed: $reply"; exit 1; }

# The page's own account of its geometry.
geo_raw="$(output "$(cmd execute_js '"args":{"code":"window.lurienGeometry()"}')")"
printf '%s\n' "$geo_raw" > "$work/geo.txt"
read -r dpr inner_w inner_h scroll_w scroll_h target_x target_y target_w target_h snap_w snap_h <<EOF
$(python3 - "$work/geo.txt" <<'PY'
import json, sys
raw = open(sys.argv[1]).read().strip()
# The eval face returns the script's value; it may arrive JSON-quoted.
while raw and raw[0] == '"':
    raw = json.loads(raw)
g = json.loads(raw)
t, s = g["target"], g["snap"]
print(g["dpr"], g["inner_width"], g["inner_height"], g["scroll_width"], g["scroll_height"],
      t["x"], t["y"], t["width"], t["height"], s["width"], s["height"])
PY
)
EOF
if [ -z "${dpr:-}" ]; then
  echo "FAIL: the page did not report its geometry: $geo_raw"
  cat "$work/serve.log"
  exit 1
fi

# Claim 1: the viewport.
viewport="$(dims "$(output "$(cmd dom_screenshot)")")"
close_to "$viewport" "$inner_w" "$inner_h" "$dpr" \
  || fail "viewport capture is $viewport, not ${inner_w}x${inner_h} at dpr $dpr"

# Claim 2: the whole document.
fullpage="$(dims "$(output "$(cmd dom_screenshot '"args":{"full_page":"true"}')")")"
close_to "$fullpage" "$scroll_w" "$scroll_h" "$dpr" \
  || fail "full-page capture is $fullpage, not ${scroll_w}x${scroll_h} at dpr $dpr"
python3 -c 'import sys; f=sys.argv[1]; v=sys.argv[2]; sys.exit(0 if int(f.split("x")[1]) > int(v.split("x")[1]) else 1)' \
  "$fullpage" "$viewport" || fail "full-page capture ($fullpage) is no taller than the viewport ($viewport)"

# Claim 3: a rectangle below the fold, captured without scrolling, holding the
# pixels that live at that rectangle.
clip_spec="$target_x,$target_y,$target_w,$target_h"
clipped="$(dims "$(output "$(cmd dom_screenshot "\"args\":{\"clip\":\"$clip_spec\",\"path\":\"$work/clip.png\"}")")")"
close_to "$clipped" "$target_w" "$target_h" "$dpr" \
  || fail "clip $clip_spec captured $clipped, not ${target_w}x${target_h} at dpr $dpr"
centre_is "$work/clip.png" 204a87 \
  || fail "the clip is the right size but not the right rectangle: $(centre_is "$work/clip.png" 204a87)"
scrolled="$(output "$(cmd execute_js '"args":{"code":"JSON.stringify(window.scrollY)"}')")"
case "$scrolled" in
  *0*) : ;;
  *) fail "capturing a rectangle scrolled the page to ${scrolled:-nowhere it would admit}" ;;
esac

# Claim 4: one element, by CSS and by a semantic description. Taken while the
# page is scrolled, so an element rectangle measured in viewport coordinates
# instead of document coordinates lands on the wrong band and shows it.
cmd execute_js '"args":{"code":"JSON.stringify(window.scrollTo(0, 1500) || window.scrollY)"}' >/dev/null
by_css="$(dims "$(output "$(cmd dom_screenshot "\"args\":{\"selector\":\"#target\",\"path\":\"$work/element.png\"}")")")"
close_to "$by_css" "$target_w" "$target_h" "$dpr" \
  || fail "selector #target captured $by_css, not ${target_w}x${target_h} at dpr $dpr"
[ "$by_css" = "$clipped" ] \
  || fail "the element capture ($by_css) and its own rectangle ($clipped) disagree"
centre_is "$work/element.png" 204a87 \
  || fail "the element capture landed elsewhere: $(centre_is "$work/element.png" 204a87)"
by_role="$(dims "$(output "$(cmd dom_screenshot '"args":{"selector":"role:button=Snap"}')")")"
close_to "$by_role" "$snap_w" "$snap_h" "$dpr" \
  || fail "selector role:button=Snap captured $by_role, not ${snap_w}x${snap_h} at dpr $dpr"
cmd execute_js '"args":{"code":"JSON.stringify(window.scrollTo(0, 0) || window.scrollY)"}' >/dev/null

# Claim 5: a frame's own document, at what the frame shows and at its full size.
inner_geo="$(output "$(cmd execute_js '"args":{"code":"JSON.stringify({cw: document.documentElement.clientWidth, ch: document.documentElement.clientHeight, sw: document.documentElement.scrollWidth, sh: document.documentElement.scrollHeight})","frame":"url:shot_frame.html"}')")"
printf '%s\n' "$inner_geo" > "$work/inner.txt"
read -r frame_cw frame_ch frame_sw frame_sh <<EOF
$(python3 - "$work/inner.txt" <<'PY'
import json, sys
raw = open(sys.argv[1]).read().strip()
while raw and raw[0] == '"':
    raw = json.loads(raw)
g = json.loads(raw)
print(g["cw"], g["ch"], g["sw"], g["sh"])
PY
)
EOF
[ -n "${frame_cw:-}" ] || { echo "FAIL: the inner frame did not report its box: $inner_geo"; exit 1; }
[ "$frame_sw" = "500" ] && [ "$frame_sh" = "360" ] \
  || fail "the inner document is ${frame_sw}x${frame_sh}, so the fixture no longer says what this test asserts"
frame_view="$(dims "$(output "$(cmd dom_screenshot '"args":{"frame":"url:shot_frame.html"}')")")"
close_to "$frame_view" "$frame_cw" "$frame_ch" "$dpr" \
  || fail "frame capture is $frame_view, not the ${frame_cw}x${frame_ch} the frame shows at dpr $dpr"
frame_full="$(dims "$(output "$(cmd dom_screenshot "\"args\":{\"frame\":\"url:shot_frame.html\",\"full_page\":\"true\",\"path\":\"$work/frame.png\"}")")")"
close_to "$frame_full" "$frame_sw" "$frame_sh" "$dpr" \
  || fail "frame document capture is $frame_full, not ${frame_sw}x${frame_sh} at dpr $dpr"
centre_is "$work/frame.png" 7a3e9d \
  || fail "the frame capture is not the frame's document: $(centre_is "$work/frame.png" 7a3e9d)"
python3 -c 'import sys; a=sys.argv[1]; b=sys.argv[2]; sys.exit(0 if a != b else 1)' "$frame_view" "$frame_full" \
  || fail "the frame's visible box and its whole document captured the same size ($frame_view)"

# Claim 6: refusals name the correction.
both="$(error_of "$(cmd dom_screenshot '"args":{"full_page":"true","clip":"0,0,10,10"}')")"
case "$both" in
  *"name a different area"*) : ;;
  *) fail "two areas at once was not refused: $both" ;;
esac
malformed="$(error_of "$(cmd dom_screenshot '"args":{"clip":"10,20,wide,40"}')")"
case "$malformed" in
  *"x,y,width,height"*) : ;;
  *) fail "a malformed rectangle was not refused with the shape it wanted: $malformed" ;;
esac
boxless="$(error_of "$(cmd dom_screenshot '"args":{"selector":"#collapsed","timeout_ms":"1200"}')")"
case "$boxless" in
  *"no pixels"*) : ;;
  *) fail "an element with no box was not refused: $boxless" ;;
esac

# A capture written to disk is a real PNG of the same size.
saved="$work/full.png"
cmd dom_screenshot "\"args\":{\"full_page\":\"true\",\"path\":\"$saved\"}" >/dev/null
python3 - "$saved" "$fullpage" <<'PY' || fail "the saved PNG is not the capture it reported"
import struct, sys
data = open(sys.argv[1], "rb").read()
want_w, want_h = (int(v) for v in sys.argv[2].split("x"))
assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
w, h = struct.unpack(">II", data[16:24])
assert (w, h) == (want_w, want_h), f"file is {w}x{h}, reported {want_w}x{want_h}"
PY

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: viewport $viewport, document $fullpage, rectangle $clipped without scrolling, element $by_css and $by_role, frame $frame_view then $frame_full, and three impossible areas refused"

#!/usr/bin/env bash
# The slider kind, end to end, against a puzzle whose notch moves every load.
#
# Three claims, three phases:
#
#   1. The offset is read from the image. The notch is minted per load, so a
#      constant cannot land within the fixture's three-pixel tolerance.
#   2. The travel is a hand's. The fixture refuses a drag with fewer than eight
#      moves, with constant step size, with no correction, or under 80 ms, so a
#      driver-side dragAndDrop is refused even when it lands on the right pixel.
#   3. A linear travel is refused. The same run with an evenly spaced profile must
#      not produce a token, which is what proves phase 2 is a real gate.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_slider.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
vision="${LURIEN_VISION:-$target/debug/lurien-vision}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
helper_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
# Loopback reaches every process on this host, so the helper is private only for as
# long as this token is.
helper_token="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  [[ -n "${helper_pid:-}" ]] && kill "$helper_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }
[[ -x "$vision" ]] || { echo "FAIL: no vision helper at $vision (cargo build -p lurien-vision)"; exit 1; }

mkdir -p "$work/www"
# The page is served from localhost, the widget from 127.0.0.1: same server, two
# hosts, so the widget is cross-origin and gets its own context and process.
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_slider.html|" \
  "$fixtures/challenge_slider_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_slider.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

# The helper is a separate process on loopback. It sees one crop and nothing else,
# and it answers only for this session's token: loopback is not access control.
"$vision" --port "$helper_port" --token "$helper_token" > "$work/helper.log" 2>&1 &
helper_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }
for _ in $(seq 1 40); do
  if python3 -c "
import socket, sys
s = socket.socket()
s.settimeout(0.3)
sys.exit(0 if s.connect_ex(('127.0.0.1', $helper_port)) == 0 else 1)
"; then break; fi
  sleep 0.25
done

# The binding names the two elements the way the vendor files do: the puzzle is a
# canvas and the handle is whatever the widget paints as draggable. A fixture that
# named its own ids would prove nothing about the shipped catalog.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "slider",
      "source": "e2e_slider.sh",
      "target": "first canvas in this BC",
      "handle": "first draggable in this BC",
      "iframe_src": ["challenge_slider.html"],
      "custom_elements": [],
      "selectors": ["#puzzle"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 12000,
  "claimed_kinds": ["none", "score", "checkbox", "pow", "slider", "fail"],
  "poll_ms": 200,
  "helper": { "host": "127.0.0.1", "port": $helper_port, "token": "$helper_token" }
}
JSON

export LURIEN_CHALLENGE="$(cat "$work/config.json")"
set +e
out="$("$lurien" --headless goto "http://localhost:$port/parent.html" 2>&1)"
status=$?
set -e
echo "goto: $out"
echo "helper: $(cat "$work/helper.log")"

[[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for the fixture page"; exit 1; }
echo "evidence: $(cat "$evidence")"

python3 - "$evidence" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)
need(row["kind"] == "slider", "the engine did not classify the page as a slider")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the fixture refused the drag: wrong offset, or a travel no hand would make")
need(row["via"] == "field", "success was not observed as a token write")
need(row["source"] == "engine", "the solve did not come from the engine")
work = row.get("work") or {}
need(float(work.get("dx", 0)) > 40, "the helper reported no meaningful travel")
need(int(work.get("moves", 0)) >= 8, "the drag was dispatched as too few moves to be a hand")
print(
    f"ok: slider solved by dragging {work['dx']:.1f}px in {work['moves']} moves "
    f"(confidence {work.get('confidence')}), token via {row['via']} in {row['ms']}ms"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"slider"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

# Phase 3: the same offset, applied as a straight constant-speed travel. The
# fixture must refuse it, which is what makes phase 2 evidence rather than decor.
linear="$work/linear.jsonl"
python3 - "$work/config.json" "$work/linear-config.json" "$linear" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
# Ten evenly spaced steps, one dwell, no overshoot: exactly what a driver-side
# dragAndDrop produces.
config["drag_profile"] = [{"f": (i + 1) / 10, "dy": 0, "dt": 10} for i in range(10)]
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/linear-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$linear" ]] || { echo "FAIL: the engine wrote no evidence for the linear travel"; exit 1; }
python3 - "$linear" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
if row["solved"] is not False:
    print(f"FAIL: the fixture accepted a straight constant-speed drag: {row}")
    sys.exit(1)
print(f"ok: a linear travel was refused: {row['error']}")
PY

# Phase 4: the helper's own door. Loopback reaches every process on this host, so
# a helper that answers a line without this session's token is a perception service
# anything local can queue work on and read answers from.
python3 - "$helper_port" "$helper_token" <<'PY'
import json, socket, sys

port, token = int(sys.argv[1]), sys.argv[2]
crop = {"kind": "slider", "task": "axis", "png": "", "width": 300, "height": 65}


def ask(request):
    s = socket.create_connection(("127.0.0.1", port), timeout=5)
    s.sendall((json.dumps(request) + "\n").encode())
    reply = s.makefile("r").readline()
    s.close()
    return json.loads(reply)


for name, request in [
    ("no token", {"v": 1, **crop}),
    ("empty token", {"v": 1, "token": "", **crop}),
    ("wrong token", {"v": 1, "token": "0" * len(token), **crop}),
    ("no version", {"token": token, **crop}),
]:
    reply = ask(request)
    if "error" not in reply or "dx" in reply:
        print(f"FAIL: the helper answered a request with {name}: {reply}")
        sys.exit(1)

reply = ask({"v": 1, "token": token, **crop})
if reply.get("error") is None or "png" not in reply["error"]:
    print(f"FAIL: an authenticated request was not read as a request: {reply}")
    sys.exit(1)
print("ok: the helper refused every unauthenticated line and read the authenticated one")
PY

echo "PASS: the engine measured the notch, dragged the handle like a hand, was refused when it did not, and the helper refused every line without this session's token"

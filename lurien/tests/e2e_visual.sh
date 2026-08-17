#!/usr/bin/env bash
# The visual kind, end to end, against a tile grid whose question and answer are
# minted every load.
#
# Four claims, four phases:
#
#   1. The tiles are recognized. The asked-for shape, the tiles holding it, and how
#      many there are change per load, and the fixture writes its token only when
#      exactly the matching tiles were clicked, one at a time, through trusted
#      events, so no constant and no click-them-all strategy reaches a token. The
#      pacing is the card the session dealt, which the evidence row names.
#   2. Twice in a row. A second visit draws a new question and must also be solved,
#      which a remembered answer cannot do.
#   3. A question the grid does not answer is refused by name. Nothing is clicked,
#      which is what proves phase 1 was recognition and not a click on every tile.
#   4. A helper with no model refuses by name rather than guessing cells.
#
# The weights are not in this tree. With no model directory the proof skips loudly:
# a silent pass would be the same defect as a false one.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_visual.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
vision="${LURIEN_VISION:-$target/debug/lurien-vision}"
model="${LURIEN_VISION_MODEL:-$HOME/.cache/lurien/vision/owlvit-base-patch32}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
helper_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
blind_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
helper_token="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  [[ -n "${helper_pid:-}" ]] && kill "$helper_pid" 2>/dev/null || true
  [[ -n "${blind_pid:-}" ]] && kill "$blind_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }
[[ -x "$vision" ]] || { echo "FAIL: no vision helper at $vision (cargo build -p lurien-vision)"; exit 1; }
if [[ ! -f "$model/model.onnx" ]]; then
  echo "SKIP: no grid detector at $model; set LURIEN_VISION_MODEL to an open-vocabulary detector ONNX export"
  exit 0
fi

mkdir -p "$work/www"
# The page is served from localhost, the widget from 127.0.0.1: same server, two
# hosts, so the widget is cross-origin and gets its own context and process.
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_visual.html|" \
  "$fixtures/challenge_visual_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_visual.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

# The helper sees one crop, the question as words, and the rectangles the browser
# laid out. It answers with indices, so no page coordinate leaves the browser.
"$vision" --port "$helper_port" --token "$helper_token" --model "$model" > "$work/helper.log" 2>&1 &
helper_pid=$!
# The same binary with no model, for phase 4.
"$vision" --port "$blind_port" --token "$helper_token" > "$work/blind.log" 2>&1 &
blind_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }
for p in "$helper_port" "$blind_port"; do
  for _ in $(seq 1 40); do
    if python3 -c "
import socket, sys
s = socket.socket()
s.settimeout(0.3)
sys.exit(0 if s.connect_ex(('127.0.0.1', $p)) == 0 else 1)
"; then break; fi
    sleep 0.25
  done
done

# The binding names the widget's parts the way a vendor file does: a grid table
# with the question, the tile selector, and the control that confirms the set.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "visual",
      "source": "e2e_visual.sh",
      "target": "#grid",
      "grid": { "prompt": "#prompt", "cell": ".tile", "submit": "#verify" },
      "iframe_src": ["challenge_visual.html"],
      "custom_elements": [],
      "selectors": ["#grid"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 30000,
  "kind_budget_ms": { "visual": 40000 },
  "claimed_kinds": ["none", "score", "checkbox", "pow", "slider", "visual", "fail"],
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
need(row["kind"] == "visual", "the engine did not classify the page as a tile grid")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the fixture refused the set: wrong tiles, untrusted clicks, or one gesture")
need(row["via"] == "field", "success was not observed as a token write")
need(row["source"] == "engine", "the solve did not come from the engine")
work = row.get("work") or {}
cells = work.get("cells") or []
need(2 <= len(cells) <= 4, f"the engine clicked {len(cells)} tiles, and the fixture plants two to four")
need(work.get("of") == 9, f"the browser reported {work.get('of')} tiles for a three by three grid")
need("a photo of a" not in (work.get("prompt") or ""), "the widget's own words were replaced by the helper's caption")
need(bool(work.get("prompt")), "the question the widget asked was not recorded")
need(isinstance((row.get("dyn") or {}).get("grid"), int), "the answer's pacing was not dealt from the session deck")
print(
    f"ok: {len(cells)} of {work['of']} tiles clicked for {work['prompt']!r} "
    f"(confidence {work.get('confidence')}), token via {row['via']} in {row['ms']}ms"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"visual"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

# Phase 2: a second visit. The fixture draws a new question, a new count and a new
# layout, so an answer that worked once cannot be reused.
again="$work/again.jsonl"
python3 - "$work/config.json" "$work/again-config.json" "$again" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/again-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$again" ]] || { echo "FAIL: the engine wrote no evidence for the second visit"; exit 1; }
python3 - "$again" "$evidence" <<'PY'
import json, sys
def last(path):
    return [json.loads(line) for line in open(path) if line.strip()][-1]
row, first = last(sys.argv[1]), last(sys.argv[2])
if row["solved"] is not True:
    print(f"FAIL: the second visit was not solved: {row}")
    sys.exit(1)
work, before = row.get("work") or {}, first.get("work") or {}
print(
    f"ok: a second question was answered: {before.get('prompt')!r} -> {work.get('prompt')!r}, "
    f"cells {before.get('cells')} -> {work.get('cells')}"
)
PY

# Phase 3: a question the grid does not answer. The fixture carries a second
# question naming a shape it never draws, so a binding pointed at it asks for
# something that is not there. Nothing may be clicked, and the refusal has to name
# the question: a solver that picks a tile anyway is guessing, and a vendor grades a
# grid all or nothing.
absent="$work/absent.jsonl"
python3 - "$work/config.json" "$work/absent-config.json" "$absent" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
config["catalog"][0]["grid"]["prompt"] = "#decoy"
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/absent-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$absent" ]] || { echo "FAIL: the engine wrote no evidence for the unanswerable question"; exit 1; }
python3 - "$absent" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
if row["solved"] is not False:
    print(f"FAIL: a question the grid does not answer produced a token: {row}")
    sys.exit(1)
error = row.get("error") or ""
if "no tile" not in error or "yellow star" not in error:
    print(f"FAIL: the refusal does not name the question that went unanswered: {error}")
    sys.exit(1)
print(f"ok: an unanswerable question was refused: {error}")
PY

# Phase 4: the same helper binary with no weights. A grid it cannot see is refused
# by name, and the refusal names what to pass.
blind="$work/blind.jsonl"
python3 - "$work/config.json" "$work/blind-config.json" "$blind" "$blind_port" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
config["helper"]["port"] = int(sys.argv[4])
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/blind-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$blind" ]] || { echo "FAIL: the engine wrote no evidence for the model-less helper"; exit 1; }
python3 - "$blind" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
if row["solved"] is not False:
    print(f"FAIL: a helper with no model reported a solve: {row}")
    sys.exit(1)
error = row.get("error") or ""
if "grid classifier" not in error or "--model" not in error:
    print(f"FAIL: the refusal does not name the missing model or how to pass one: {error}")
    sys.exit(1)
print(f"ok: a helper with no weights refused by name: {error}")
PY

echo "PASS: the engine read the question, recognized the tiles, clicked them at a hand's pace, was refused when it did not, and a helper with no model refused rather than guessed"

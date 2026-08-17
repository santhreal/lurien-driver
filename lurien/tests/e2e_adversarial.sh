#!/usr/bin/env bash
# One page that holds every line a scripted solver crosses.
#
# The other fixtures each prove one thing. This one arms four refusals at once and
# writes its token only when all four held, so a solve here is evidence about all
# of them: an untrusted click is refused (the widget clicks itself at load and
# latches if that worked), a press without six trusted moves and 120 ms of approach
# is refused, a token the widget did not write itself is a forgery and latches, and
# a page that turns out to be able to read the widget makes the widget refuse
# outright.
#
# Four phases:
#
#   1. the engine solves it. Every refusal above was armed and none of them fired.
#   2. a page with no widget on it must be `none`, and must not be acted on. A
#      solver that reports work on a page that had none is worse than one that
#      fails.
#   3. the same widget with a page script writing a plausible token 200 ms after
#      load. The widget latches and the run is refused, which is what stops a
#      solver from being believed when it writes the token itself.
#   4. one exact `trajectory` of a single point: a teleport to the centre. The
#      click is trusted and lands, and the widget refuses it for arriving without
#      an approach.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_adversarial.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }

# The page is served from localhost, the widget from 127.0.0.1: one server, two
# hosts, so the widget is cross-origin and gets its own context and process.
mkdir -p "$work/www"
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_adversarial_child.html|" \
  "$fixtures/challenge_adversarial_parent.html" > "$work/www/parent.html"
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_adversarial_child.html?forge=1|" \
  "$fixtures/challenge_adversarial_parent.html" > "$work/www/forged.html"
cp "$fixtures/challenge_adversarial_child.html" "$work/www/"
cp "$fixtures/locator_forms.html" "$work/www/plain.html"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null \
  || { echo "FAIL: fixture server never came up"; exit 1; }

# One binding. The engine learns the vendor from data, so this fixture is a vendor
# as far as it can tell. The fourth phase adds an exact trajectory, which outranks
# the deck the driver would otherwise fill in.
config() {
  local name="$1"
  local extra="$2"
  cat > "$work/$name-config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_adversarial.sh",
      "target": "first checkbox in this BC",
      "iframe_src": ["challenge_adversarial_child.html"],
      "custom_elements": [],
      "selectors": [],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": [],
      "token_storage": [],
      "token_messages": []
    }
  ],
  "evidence": "$work/$name.jsonl",
  "budget_ms": 25000,
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
  "poll_ms": 200$extra
}
JSON
}

visit() {
  local name="$1"
  local page="$2"
  set +e
  out="$(LURIEN_CHALLENGE="$(cat "$work/$name-config.json")" \
    "$lurien" --headless goto "http://localhost:$port/$page" 2>&1)"
  status=$?
  set -e
}

# Phase 1. Every refusal armed, none of them fired.
config armed ""
visit armed parent.html
echo "goto (armed): $out"
[[ -s "$work/armed.jsonl" ]] || { echo "FAIL: the engine wrote no evidence"; exit 1; }
echo "evidence (armed): $(cat "$work/armed.jsonl")"
python3 - "$work/armed.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict: {rows}")
    sys.exit(1)
row = verdicts[-1]


def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


need(row["kind"] == "checkbox", "the engine did not classify the widget as a checkbox")
need(row["solved"] is True, "the widget refused: one of its four lines was crossed")
need(row["via"] == "field", "success was not observed as a token write")
need(row["contexts"] >= 2, "the observer did not attach to the widget's own context")
need(row["source"] == "engine", "the solve did not come from the engine")
visit = row.get("visit") or {}
need(int(visit.get("moves", 0)) >= 6, "the visit did not carry the motion the widget requires")
print(f"ok: four refusals armed, solved via {row['via']} in {row['ms']}ms across {row['contexts']} contexts")
PY
if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

# Phase 2. A page with no widget is `none` and nothing is acted on.
config plain ""
visit plain plain.html
echo "goto (plain): $out"
if [[ $status -ne 0 ]]; then
  echo "FAIL: a page with no challenge exited $status"
  exit 1
fi
case "$out" in
  *'"kind":"none"'*) ;;
  *) echo "FAIL: a page with no challenge was not reported as none: $out"; exit 1 ;;
esac
if [[ -s "$work/plain.jsonl" ]]; then
  python3 - "$work/plain.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
acted = [r for r in rows if "solved" in r]
if acted:
    print(f"FAIL: the engine acted on a page with no challenge: {acted[-1]}")
    sys.exit(1)
print("ok: a page with no widget produced no verdict to report")
PY
else
  echo "ok: a page with no widget produced no evidence at all"
fi

# Phase 3. A page script writes a plausible token while the engine works. The
# widget latches, so the run must be refused rather than believed.
config forged ""
visit forged forged.html
echo "goto (forged): $out"
if [[ $status -eq 0 ]]; then
  echo "FAIL: a token written by page script was accepted as a solve"
  exit 1
fi
[[ -s "$work/forged.jsonl" ]] || { echo "FAIL: the engine wrote no evidence for the forged token"; exit 1; }
echo "evidence (forged): $(cat "$work/forged.jsonl")"
python3 - "$work/forged.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the forged token: {rows}")
    sys.exit(1)
row = verdicts[-1]
if row["solved"] is not False:
    print(f"FAIL: a forged token was reported as a solve: {row}")
    sys.exit(1)
if not (row.get("error") or ""):
    print(f"FAIL: the refusal names no reason: {row}")
    sys.exit(1)
print(f"ok: a token the widget did not write was refused: {row['error']}")
PY

# Phase 4. One point of trajectory: the pointer appears at the centre and presses.
# The click is trusted and it still must not clear the widget.
config teleport ',
  "trajectory": [{ "x": 0.5, "y": 0.5, "dt": 0 }],
  "prelude": {}'
visit teleport parent.html
echo "goto (teleport): $out"
if [[ $status -eq 0 ]]; then
  echo "FAIL: a trusted click with no approach was accepted"
  exit 1
fi
[[ -s "$work/teleport.jsonl" ]] || { echo "FAIL: the engine wrote no evidence for the teleport"; exit 1; }
echo "evidence (teleport): $(cat "$work/teleport.jsonl")"
python3 - "$work/teleport.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the teleport: {rows}")
    sys.exit(1)
row = verdicts[-1]
if row["solved"] is not False:
    print(f"FAIL: a click with no approach was reported as a solve: {row}")
    sys.exit(1)
print(f"ok: a trusted click with no approach was refused: {row['error']}")
PY

echo "PASS: four refusals armed and cleared, a page with no widget left alone, a forged token and an approachless click both refused"

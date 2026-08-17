#!/usr/bin/env bash
# Typed answers have a rhythm.
#
# The fixture is a proof-of-work page that also measures how its answer arrived:
# it counts trusted key events, measures how long each key was held, and refuses
# an answer whose keys were all held the same length of time or typed at one
# constant gap. Those are the two numbers a keystroke-dynamics classifier reads,
# and they are what a browser handed a string produces when it types as fast as it
# can.
#
# Three phases:
#
#   1. the session's own typing model. The driver samples a gap per pair class and
#      a hold per character class from the persona's typing corpus, ships them as a
#      deck, and the engine deals one per keystroke and classifies each digraph.
#      The page must accept, which it only does when the holds and the gaps vary.
#   2. one explicit plan, constant gap and no hold, which is the shape before this
#      model existed. The page must refuse it. Without this phase the first proves
#      only that the page accepts something.
#   3. a deck of two entries that differ in hold and agree in gap. Every structural
#      gate passes and only the dispersion of the gaps refuses it, so this is the
#      phase that says the rhythm, not merely the key press, is what was measured.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_keys.sh
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

mkdir -p "$work/www"
cp "$fixtures/challenge_typing.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://127.0.0.1:$port/challenge_typing.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://127.0.0.1:$port/challenge_typing.html" >/dev/null \
  || { echo "FAIL: fixture server never came up"; exit 1; }

# One binding, reused by both phases. The second phase adds a `keys` plan, which
# outranks the deck the driver would otherwise fill in.
config() {
  local name="$1"
  local extra="$2"
  cat > "$work/$name-config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "pow",
      "source": "e2e_keys.sh",
      "target": "input[name=pow-response]",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["input[name=pow-response]"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": [],
      "token_storage": [],
      "token_messages": [],
      "work": {
        "algo": "sha256",
        "format": "hex-zeros",
        "challenge": "global:__pow.challenge",
        "difficulty": "global:__pow.difficulty",
        "submit": "field:input[name=pow-response]"
      }
    }
  ],
  "evidence": "$work/$name.jsonl",
  "budget_ms": 30000,
  "claimed_kinds": ["none", "score", "checkbox", "pow", "fail"],
  "poll_ms": 200$extra
}
JSON
}

visit() {
  local name="$1"
  set +e
  out="$(LURIEN_CHALLENGE="$(cat "$work/$name-config.json")" \
    "$lurien" --headless goto "http://127.0.0.1:$port/challenge_typing.html" 2>&1)"
  status=$?
  set -e
}

# Phase 1. A nonce shorter than three digits carries no rhythm and the page says
# so; a fresh load mints a fresh challenge, so that run is retried rather than
# read as a verdict about typing.
config modelled ""
solved=0
for attempt in 1 2 3; do
  rm -f "$work/modelled.jsonl"
  visit modelled
  echo "goto (attempt $attempt): $out"
  [[ -s "$work/modelled.jsonl" ]] || { echo "FAIL: the engine wrote no evidence"; exit 1; }
  if python3 - "$work/modelled.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
sys.exit(0 if verdicts and verdicts[-1]["solved"] else 1)
PY
  then
    solved=1
    break
  fi
  echo "note: the page did not accept that answer, retrying with a fresh challenge"
done
[[ $solved -eq 1 ]] || { echo "FAIL: three answers were refused: $(cat "$work/modelled.jsonl")"; exit 1; }

echo "evidence (modelled): $(cat "$work/modelled.jsonl")"
python3 - "$work/modelled.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
row = [r for r in rows if "solved" in r][-1]


def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


need(row["kind"] == "pow", "the engine did not classify the page as pow")
need(row["solved"] is True, "the page refused the way the answer was typed")
need(row["via"] == "field", "the token did not arrive in the answer field")
work = row.get("work") or {}
need(work.get("submitted") == "field", "the answer did not go in through the keyboard path")
print(f"ok: the page accepted an answer of {work.get('difficulty')} hex zeros typed by the model")
PY

# Phase 2. One gap for every pair and no hold at all: the cadence this model
# exists to replace. The page must refuse it, and the refusal must be the engine's
# typed error rather than a silent pass.
config constant ',
  "keys": { "gap": { "hot": 60, "cold": 60, "space": 60, "digit": 60 }, "hold": { "lower": 0, "upper": 0, "digit": 0, "space": 0, "other": 0 } }'
visit constant
echo "goto (constant): $out"
if [[ $status -eq 0 ]]; then
  echo "FAIL: a constant cadence with no key hold was accepted"
  exit 1
fi
[[ -s "$work/constant.jsonl" ]] || { echo "FAIL: the engine wrote no evidence for the constant plan"; exit 1; }
echo "evidence (constant): $(cat "$work/constant.jsonl")"
python3 - "$work/constant.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the constant plan: {rows}")
    sys.exit(1)
row = verdicts[-1]
if row["solved"] is not False:
    print(f"FAIL: a constant cadence was reported as a solve: {row}")
    sys.exit(1)
if not (row.get("error") or ""):
    print(f"FAIL: the refusal names no reason: {row}")
    sys.exit(1)
print(f"ok: a constant cadence with no hold was refused: {row['error']}")
PY

# Phase 3. Keys that are held, and held for different lengths, but typed at one
# rate. This is the plan that passes every structural gate: the holds vary, no gap
# is impossibly short, and only the dispersion of the gaps says the rhythm was
# generated. A caller-supplied deck is honoured as-is, so this ships two entries
# whose gap is the same number twice and whose holds differ by just enough to pass
# the page's hold gate, which leaves the dispersion of the gaps as the only reason
# the page can refuse.
config rate ',
  "key_deck": [
    { "gap": { "hot": 250, "cold": 250, "space": 250, "digit": 250 }, "hold": { "lower": 40, "upper": 40, "digit": 40, "space": 40, "other": 40 } },
    { "gap": { "hot": 250, "cold": 250, "space": 250, "digit": 250 }, "hold": { "lower": 46, "upper": 46, "digit": 46, "space": 46, "other": 46 } }
  ]'
visit rate
echo "goto (rate): $out"
if [[ $status -eq 0 ]]; then
  echo "FAIL: one constant typing rate was accepted"
  exit 1
fi
[[ -s "$work/rate.jsonl" ]] || { echo "FAIL: the engine wrote no evidence for the constant rate"; exit 1; }
echo "evidence (rate): $(cat "$work/rate.jsonl")"
python3 - "$work/rate.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the constant rate: {rows}")
    sys.exit(1)
row = verdicts[-1]
if row["solved"] is not False:
    print(f"FAIL: keys held well but typed at one rate were reported as a solve: {row}")
    sys.exit(1)
if not (row.get("error") or ""):
    print(f"FAIL: the refusal names no reason: {row}")
    sys.exit(1)
print(f"ok: one typing rate was refused even with the keys held: {row['error']}")
PY

echo "PASS: the answer was typed with the session's own rhythm; a constant cadence and a constant rate were both refused"

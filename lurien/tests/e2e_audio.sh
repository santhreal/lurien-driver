#!/usr/bin/env bash
# The audio kind, end to end, against a widget whose answer exists only as sound.
#
# Four claims, four phases:
#
#   1. The recording is heard. The digits are minted per load and spoken by the
#      host's synthesizer under the noise a vendor's clip carries; the page holds
#      only their SHA-256 and grades in the page, so the answer is nowhere in the
#      DOM and cannot be read instead of heard. The clip appears only after a
#      trusted press of the play control, and the answer is accepted only when it
#      was typed key by key at a hand's pace.
#   2. Twice in a row. A second visit mints a new code, which a remembered
#      transcript cannot answer.
#   3. A recording that says nothing is refused, not guessed. The noise mode serves
#      the same noise with no speech under it, and the run has to end with nothing
#      typed and a refusal naming the floor.
#   4. A helper with no speech model refuses by name rather than inventing digits.
#
# The weights are not in this tree, and the voice is the host's. With no model or no
# synthesizer this skips loudly: a silent pass is the same defect as a false one.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_audio.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
vision="${LURIEN_VISION:-$target/debug/lurien-vision}"
model="${LURIEN_AUDIO_MODEL:-$HOME/.cache/lurien/audio/whisper-small.en}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
helper_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
deaf_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
helper_token="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  [[ -n "${helper_pid:-}" ]] && kill "$helper_pid" 2>/dev/null || true
  [[ -n "${deaf_pid:-}" ]] && kill "$deaf_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }
[[ -x "$vision" ]] || { echo "FAIL: no helper at $vision (cargo build -p lurien-vision)"; exit 1; }
if [[ ! -f "$model/encoder_model.onnx" ]]; then
  echo "SKIP: no speech model at $model; set LURIEN_AUDIO_MODEL to a Whisper ONNX export"
  exit 0
fi
if ! command -v espeak-ng >/dev/null 2>&1; then
  echo "SKIP: no espeak-ng on this host, so the fixture cannot speak its own code"
  exit 0
fi

# The page is served from localhost, the widget from 127.0.0.1: one server, two
# hosts, so the widget is cross-origin and gets its own context and process. The
# recording is minted per nonce by the fixture server, not committed.
python3 "$here/audio_fixture.py" --port "$port" --fixtures "$fixtures" \
  --child-host 127.0.0.1 >"$work/fixture.log" 2>&1 &
server_pid=$!

# The helper is handed base64 audio and has no network: the bytes were fetched by
# the widget's own context, with the widget's own cookies and referrer.
"$vision" --port "$helper_port" --token "$helper_token" --audio "$model" \
  > "$work/helper.log" 2>&1 &
helper_pid=$!
# The same binary with no speech model, for phase 4.
"$vision" --port "$deaf_port" --token "$helper_token" > "$work/deaf.log" 2>&1 &
deaf_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }
for p in "$helper_port" "$deaf_port"; do
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

# The binding names the widget's parts the way a vendor file does: the control that
# plays the recording, the element that holds it, the field the answer is typed
# into, the button that confirms it, the control that serves a different recording,
# and the characters the answer is spelled with.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "audio",
      "source": "e2e_audio.sh",
      "target": "#widget",
      "audio": {
        "open": "#audio-play",
        "source": "#captcha-audio",
        "answer": "#audio-answer",
        "submit": "#verify",
        "reload": "#audio-reload",
        "alphabet": "0123456789"
      },
      "iframe_src": ["challenge_audio.html"],
      "custom_elements": [],
      "selectors": ["#captcha-audio"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 30000,
  "kind_budget_ms": { "audio": 120000 },
  "claimed_kinds": ["none", "score", "checkbox", "visual", "audio", "pow", "slider", "fail"],
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
need(row["kind"] == "audio", "the engine did not classify the page as an audio challenge")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the widget refused the answer: wrong digits, an assigned value, or one gesture")
need(row["via"] == "field", "success was not observed as a token write")
need(row["source"] == "engine", "the solve did not come from the engine")
work = row.get("work") or {}
readings = work.get("readings") or []
need(bool(readings), "no reading of the recording was recorded")
typed = work.get("typed") or ""
need(typed.isdigit() and len(typed) == 5, f"the answer typed was {typed!r}, and the fixture speaks five digits")
need(all(c in "0123456789" for c in typed), "a character outside the declared alphabet was typed")
last = readings[-1]
need(last["text"] == typed, "the transcript that was typed is not the last reading recorded")
need(last["confidence"] >= work["floor"], f"a reading under the floor was typed: {last}")
need(last["agreement"] == 3, f"the reading was not agreed by three passes: {last}")
print(
    f"ok: heard {typed!r} at confidence {last['confidence']} with agreement {last['agreement']} "
    f"on round {work.get('round')} of {len(readings)}, token via {row['via']} in {row['ms']}ms"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"audio"'*) ;;
  *) echo "FAIL: the driver did not report the page as an audio challenge: $out"; exit 1;;
esac

# Phase 2: a second visit. The server mints a new code, so a remembered transcript
# is worth nothing.
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
second, first = last(sys.argv[1]), last(sys.argv[2])
if second["solved"] is not True:
    print(f"FAIL: the second visit was not solved: {second}")
    sys.exit(1)
one = (first.get("work") or {}).get("typed")
two = (second.get("work") or {}).get("typed")
if one == two:
    print(f"FAIL: both visits typed {one!r}, so the fixture is not minting a new code")
    sys.exit(1)
print(f"ok: {one!r} then {two!r}, two codes heard in two visits")
PY

# Phase 3: a recording that says nothing. The same noise, with no speech under it,
# for every round the solver is allowed. Nothing may be typed: a wrong answer spends
# the challenge, and a vendor offers another recording for exactly this reason.
deaf_clip="$work/noise.jsonl"
python3 - "$work/config.json" "$work/noise-config.json" "$deaf_clip" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/noise-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html?audio=noise" >/dev/null 2>&1 || true
[[ -s "$deaf_clip" ]] || { echo "FAIL: the engine wrote no evidence for the unreadable recording"; exit 1; }
python3 - "$deaf_clip" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)
need(row["kind"] == "audio", "the noise page was not classified as an audio challenge")
need(row["solved"] is False, "a recording that says nothing was reported as solved")
work = row.get("work") or {}
need(not work.get("typed"), "something was typed for a recording that says nothing")
readings = work.get("readings") or []
need(len(readings) >= 2, f"only {len(readings)} recordings were asked for before giving up")
need(str(work.get("floor")) in (row.get("error") or ""), "the refusal does not name the floor it applied")
print(f"ok: {len(readings)} recordings read and none typed: {row['error'][:120]}")
PY

# Phase 4: the same helper binary with no speech model. A recording it cannot hear
# is refused by name, and the refusal names what to pass.
deaf="$work/deaf.jsonl"
python3 - "$work/config.json" "$work/deaf-config.json" "$deaf" "$deaf_port" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
config["helper"]["port"] = int(sys.argv[4])
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/deaf-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$deaf" ]] || { echo "FAIL: the engine wrote no evidence for the model-less helper"; exit 1; }
python3 - "$deaf" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)
need(row["solved"] is False, "a helper with no speech model reported a solve")
work = row.get("work") or {}
need(not work.get("typed"), "a helper with no speech model still produced something to type")
errors = " ".join(str((r or {}).get("error") or "") for r in (work.get("readings") or []))
need("--audio" in errors or "LURIEN_AUDIO_MODEL" in errors,
     f"the refusal does not name the model it was not given: {errors!r}")
print(f"ok: refused by name: {errors[:120]}")
PY

echo "PASS: the engine pressed for the recording, read it in the widget's own context, typed what it heard at a hand's pace, asked again rather than guessing at noise, and refused without a model"

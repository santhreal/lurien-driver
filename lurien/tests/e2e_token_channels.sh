#!/usr/bin/env bash
# Success on a channel other than a form field.
#
# A vendor answers where it chose to. Two widgets here clear the same way, by a
# trusted click after real motion, and then leave the token somewhere a poller of
# form fields never looks:
#
#   1. storage. The key is written in the widget's own origin, so the page cannot
#      read it and only the widget's context can be asked.
#   2. message. The payload is posted to the page once, nested under
#      `detail.token`, and the widget keeps no copy: nothing is left to poll, so
#      the observation has to be a listener installed before the solve.
#
# Each phase requires `via` to name its channel, which is what proves the wait
# read that channel rather than falling back to a field it also happened to find.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_token_channels.sh
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

# The page is served from localhost and each widget from 127.0.0.1: same server,
# two hosts, so a widget is cross-origin and owns its own storage.
mkdir -p "$work/www"
for kind in storage message; do
  sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_${kind}_child.html|" \
    "$fixtures/challenge_checkbox_parent.html" > "$work/www/$kind.html"
  cp "$fixtures/challenge_${kind}_child.html" "$work/www/"
done
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/storage.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/storage.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# One binding per phase, naming one channel and nothing else: a binding that also
# named a field would prove nothing about the channel under test.
run_phase() {
  # One name per statement: bash expands every assignment word before `local`
  # runs, so a later word cannot read an earlier one.
  local channel="$1"
  local hook="$2"
  local page="$3"
  local evidence="$work/$channel.jsonl"
  local out=""
  local status=0
  cat > "$work/$channel-config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_token_channels.sh",
      "target": "first checkbox in this BC",
      "iframe_src": ["challenge_${channel}_child.html"],
      "custom_elements": [],
      "selectors": [],
      "cookies": [],
      "scripts": [],
      "token_inputs": [],
      "token_cookies": [],
      $hook
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 20000,
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
  "poll_ms": 200
}
JSON
  set +e
  out="$(LURIEN_CHALLENGE="$(cat "$work/$channel-config.json")" \
    "$lurien" --headless goto "http://localhost:$port/$page" 2>&1)"
  status=$?
  set -e
  echo "goto ($channel): $out"
  [[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for the $channel page"; exit 1; }
  echo "evidence ($channel): $(cat "$evidence")"
  python3 - "$evidence" "$channel" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
channel = sys.argv[2]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the {channel} page: {rows}")
    sys.exit(1)
row = verdicts[-1]


def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


need(row["kind"] == "checkbox", "the engine did not classify the widget")
need(row["solved"] is True, f"the token on the {channel} channel was never observed")
need(row["via"] == channel, f"success was reported through {row['via']}, not {channel}")
need(row["contexts"] >= 2, "the observer did not reach the widget context")
print(f"ok: the {channel} channel was observed in {row['ms']}ms across {row['contexts']} contexts")
PY
  if [[ $status -ne 0 ]]; then
    echo "FAIL: goto exited $status after the engine reported a solve on $channel"
    exit 1
  fi
}

# A named channel the vendor never writes on. The click lands, the wait reads that
# channel and nothing else, and the page is refused inside the budget it was given.
#
# This is the other half of the claim: `via` naming a channel is only worth
# something if a channel that stays empty is a refusal rather than a pass. It also
# holds the driver's own wait to the budgets it granted the engine. That wait used
# to be a fixed 25s, so the kind budget below is deliberately longer than it: the
# engine was killed mid-solve and the caller was handed the page probe's answer,
# which on a cleared checkbox is `none`. This phase costs about half a minute for
# that reason, and a shorter one would pass against the constant.
refusal_phase() {
  local evidence="$work/refused.jsonl"
  local out=""
  local status=0
  local spent=0
  cat > "$work/refused-config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_token_channels.sh",
      "target": "first checkbox in this BC",
      "iframe_src": ["challenge_message_child.html"],
      "custom_elements": [],
      "selectors": [],
      "cookies": [],
      "scripts": [],
      "token_inputs": [],
      "token_cookies": [],
      "token_storage": [],
      "token_messages": ["nope.token"]
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 6000,
  "kind_budget_ms": { "checkbox": 26000 },
  "sighting_settle_ms": 300,
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
  "poll_ms": 200
}
JSON
  local t0=$SECONDS
  set +e
  out="$(LURIEN_CHALLENGE="$(cat "$work/refused-config.json")" \
    "$lurien" --headless goto "http://localhost:$port/message.html" 2>&1)"
  status=$?
  set -e
  spent=$((SECONDS - t0))
  echo "goto (refused): $out"
  if [[ $status -eq 0 ]]; then
    echo "FAIL: goto reported a pass on a channel the page never wrote"
    exit 1
  fi
  # A third of 6000ms of reading plus 26000ms of waiting plus slack. A run that
  # took longer than this waited on something other than the config above; one that
  # ended sooner did not wait out the budget it granted.
  if [[ $spent -ge 45 ]]; then
    echo "FAIL: the refusal took ${spent}s under a 26000ms kind budget"
    exit 1
  fi
  if [[ $spent -lt 26 ]]; then
    echo "FAIL: the refusal came after ${spent}s, inside the 26000ms it granted the engine"
    exit 1
  fi
  [[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for the refused page"; exit 1; }
  echo "evidence (refused): $(cat "$evidence")"
  python3 - "$evidence" "$spent" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the refused page: {rows}")
    sys.exit(1)
row = verdicts[-1]


def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


need(row["kind"] == "checkbox", "the engine did not classify the widget")
need(row["solved"] is False, "an empty channel was reported as a solve")
need(row["via"] is None, f"a refusal named {row['via']} as the channel it succeeded on")
need("26000ms" in (row["error"] or ""), "the refusal does not name the budget it was given")
print(f"ok: an unwritten channel was refused in {row['ms']}ms, {sys.argv[2]}s of wall clock")
PY
}

run_phase storage '"token_storage": ["fixture-token"], "token_messages": []' storage.html
run_phase message '"token_storage": [], "token_messages": ["detail.token"]' message.html
refusal_phase

echo "PASS: a storage token, a posted token, and an unwritten channel were each read as what they are"

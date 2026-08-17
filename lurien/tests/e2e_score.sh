#!/usr/bin/env bash
# The class of challenge nobody clicks.
#
# A scoring vendor decides on evidence the session already produced and writes its
# token on its own schedule, in its own browsing context. There is nothing to press
# and nothing for the page to read, so the whole solve is being a session worth
# passing and then observing the write where the vendor made it. The fixture refuses
# a session that touched it, which is what turns a misclassification into a refusal
# instead of a pass that happened to work anyway.
#
# Three phases:
#
#   1. the write is observed. `kind` is `score`, `via` names the field, the observer
#      attached to both contexts, and the widget was never touched.
#   2. the widget answers later than the budget the kind was given. The refusal
#      names that budget rather than reporting a page nobody cleared as usable.
#   3. the page half proves it cannot read the widget, so nothing in phase 1 could
#      have been read from the page.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_score.sh
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
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_score_child.html|" \
  "$fixtures/challenge_score_parent.html" > "$work/www/parent.html"
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_score_child.html?delay=30000|" \
  "$fixtures/challenge_score_parent.html" > "$work/www/slow.html"
cp "$fixtures/challenge_score_child.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null \
  || { echo "FAIL: fixture server never came up"; exit 1; }

# A score binding names no target: there is nothing to act on, only a channel to
# watch. The kind budget is what bounds the wait.
config() {
  local name="$1"
  local budget="$2"
  cat > "$work/$name-config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "score",
      "source": "e2e_score.sh",
      "target": "the vendor decides on its own evidence",
      "iframe_src": ["challenge_score_child.html"],
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
  "budget_ms": 20000,
  "kind_budget_ms": { "score": $budget },
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
  "poll_ms": 200
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

# Phase 1. The vendor answers inside its budget and the engine observes it.
config scored 20000
visit scored parent.html
echo "goto (scored): $out"
[[ -s "$work/scored.jsonl" ]] || { echo "FAIL: the engine wrote no evidence"; exit 1; }
echo "evidence (scored): $(cat "$work/scored.jsonl")"
python3 - "$work/scored.jsonl" <<'PY'
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


need(row["kind"] == "score", "the engine did not classify the widget as score")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the widget wrote no token, or refused because it was touched")
need(row["via"] == "field", "the write was not observed in the widget's own field")
need(row["contexts"] >= 2, "the observer did not attach to the widget's own context")
need(row["work"] is None, "a scoring page reported work that was done to it")
print(f"ok: a scoring vendor answered on its own in {row['ms']}ms across {row['contexts']} contexts")
PY
if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi
case "$out" in
  *'"kind":"score"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

# Phase 2. The vendor answers after the budget. The refusal must name the budget,
# and the page must not be reported as usable.
config slow 4000
started=$(date +%s%3N)
visit slow slow.html
elapsed=$(( $(date +%s%3N) - started ))
echo "goto (slow): $out"
if [[ $status -eq 0 ]]; then
  echo "FAIL: a page whose vendor never answered exited 0"
  exit 1
fi
[[ -s "$work/slow.jsonl" ]] || { echo "FAIL: the engine wrote no evidence for the slow vendor"; exit 1; }
echo "evidence (slow): $(cat "$work/slow.jsonl")"
python3 - "$work/slow.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the slow vendor: {rows}")
    sys.exit(1)
row = verdicts[-1]
if row["solved"] is not False:
    print(f"FAIL: a vendor that never answered was reported as a solve: {row}")
    sys.exit(1)
if "4000ms" not in (row.get("error") or ""):
    print(f"FAIL: the refusal does not name the budget the kind was given: {row}")
    sys.exit(1)
print(f"ok: refused with {row['error']}")
PY
if (( elapsed > 25000 )); then
  echo "FAIL: a 4000ms score budget took ${elapsed}ms to refuse"
  exit 1
fi
echo "ok: the refusal arrived in ${elapsed}ms"

# Phase 3. The page cannot read the widget, so phase 1 was not a page-side read.
blind="$(curl -fsS "http://localhost:$port/parent.html" | grep -c "fixtureParentCanSeeWidget")"
if [[ "$blind" -lt 1 ]]; then
  echo "FAIL: the page half no longer checks whether it can read the widget"
  exit 1
fi
python3 - "$work/scored.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
row = [r for r in rows if "solved" in r][-1]
if row["contexts"] < 2:
    print(f"FAIL: one context means the token could have been read from the page: {row}")
    sys.exit(1)
print("ok: the token was observed in a context the page cannot read")
PY

echo "PASS: a scoring vendor was observed without being touched, and a vendor that answered late was refused with its budget named"

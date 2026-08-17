#!/usr/bin/env bash
# A kind is bounded by its own budget, not by the page's.
#
# The widget here takes a trusted click and never writes a token, which is the
# state every real deployment has when the vendor is unhappy or broken. Two runs
# of the same page differ only in whether the config names a budget for
# `checkbox`: with one, the refusal arrives on that budget; without one, the wait
# runs to the flat page budget. The gap between the two is the whole feature, and
# both refusals name the number they were given.
#
# The visit is timed separately in both runs: reading the page is a property of
# the page, so a kind budget of 1500ms must not shorten the read.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_budget.sh
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

# Page on localhost, widget on 127.0.0.1: two hosts, so the widget gets its own
# browsing context and its own process.
mkdir -p "$work/www"
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_silent_child.html|" \
  "$fixtures/challenge_checkbox_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_silent_child.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# The page budget is deliberately small so the run that has no kind budget still
# finishes inside the driver's own wait.
page_budget=8000
kind_budget=1500

# $1 evidence path, $2 the kind_budget_ms object (or `null`)
run_phase() {
  local evidence="$1" table="$2"
  cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_budget.sh",
      "target": "first checkbox in this BC",
      "iframe_src": ["challenge_silent_child.html"],
      "custom_elements": [],
      "selectors": [],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": $page_budget,
  "kind_budget_ms": $table,
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
  "poll_ms": 200
}
JSON
  LURIEN_CHALLENGE="$(cat "$work/config.json")" "$lurien" --headless goto \
    "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
  [[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for phase $evidence"; exit 1; }
}

run_phase "$work/own.jsonl" "{\"checkbox\": $kind_budget}"
run_phase "$work/flat.jsonl" "null"

echo "own:  $(tail -1 "$work/own.jsonl")"
echo "flat: $(tail -1 "$work/flat.jsonl")"

python3 - "$work/own.jsonl" "$work/flat.jsonl" "$kind_budget" "$page_budget" <<'PY'
import json, sys

own_path, flat_path, kind_budget, page_budget = sys.argv[1:5]
kind_budget, page_budget = int(kind_budget), int(page_budget)


def verdict(path):
    rows = [json.loads(line) for line in open(path) if line.strip()]
    verdicts = [row for row in rows if "solved" in row]
    if not verdicts:
        print(f"FAIL: {path} holds no verdict row: {rows}")
        sys.exit(1)
    return verdicts[-1]


def need(cond, msg, row):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


own, flat = verdict(own_path), verdict(flat_path)

for row in (own, flat):
    need(row["kind"] == "checkbox", "the widget was not classified as a checkbox", row)
    need(row["solved"] is False, "a widget that wrote no token was reported as solved", row)
    need(row["contexts"] >= 2, "the observer did not reach the widget context", row)
    need(row["visit"] is not None and row["visit"]["ok"], "the page was not read before the act", row)

need(f"given {kind_budget}ms" in own["error"], "the refusal did not name the budget for this kind", own)
need(f"given {page_budget}ms" in flat["error"], "the refusal did not name the page budget", flat)
need(own["ms"] < page_budget, "the kind's own budget did not end the wait early", own)
need(
    flat["ms"] - own["ms"] > (page_budget - kind_budget) / 2,
    f"the two runs cost the same, so the kind budget bounded nothing (own {own['ms']}ms, flat {flat['ms']}ms)",
    flat,
)
need(
    own["visit"]["ms"] > kind_budget,
    "the kind budget shortened the page read, which belongs to the page",
    own,
)
print(
    f"ok: refused on its own {kind_budget}ms budget in {own['ms']}ms "
    f"(visit {own['visit']['ms']}ms), and on the flat {page_budget}ms budget in {flat['ms']}ms"
)
PY

echo "PASS: a kind is refused on its own budget while the page keeps its read and the rest of its time"

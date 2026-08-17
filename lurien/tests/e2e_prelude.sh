#!/usr/bin/env bash
# The visit before the act, end to end, against a widget that scores the page.
#
# A trusted click is necessary and not sufficient. The fixture widget writes its
# token only when the page around it was really visited: a pointer crossed it
# outside the widget rectangle, a wheel scrolled it, and time passed between the
# load and the first pointer inside the widget. None of that is visible to the
# widget, so the parent posts it across the origin boundary.
#
# Two phases:
#
#   1. A session with the sampled prelude clears it. The reading behaviour is
#      dispatched in the top document as trusted moves and wheel events, so the
#      parent counts them and the widget accepts the click that follows.
#   2. A session with an empty prelude is refused. Same page, same click, no
#      visit, which is what proves phase 1 is a gate and not decoration.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_prelude.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }

mkdir -p "$work/www"
# The page is served from localhost, the widget from 127.0.0.1: same server, two
# hosts, so the widget is cross-origin and gets its own context and process.
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_prelude_child.html|" \
  "$fixtures/challenge_prelude_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_prelude_child.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# No `prelude` key: the driver fills one from the session persona, the same way it
# fills a trajectory. A fixture that shipped its own reading pattern would prove
# nothing about what a real session sends.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_prelude.sh",
      "target": "first checkbox in this BC",
      "handle": "",
      "iframe_src": ["challenge_prelude_child.html"],
      "custom_elements": [],
      "selectors": ["#box"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 20000,
  "claimed_kinds": ["none", "score", "checkbox", "pow", "slider", "fail"],
  "poll_ms": 200
}
JSON

export LURIEN_CHALLENGE="$(cat "$work/config.json")"
set +e
out="$("$lurien" --headless goto "http://localhost:$port/parent.html" 2>&1)"
status=$?
set -e
echo "goto: $out"

[[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for the fixture page"; exit 1; }
echo "evidence: $(cat "$evidence")"

python3 - "$evidence" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
row = [r for r in rows if "solved" in r][-1]
def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)
need(row["kind"] == "checkbox", "the engine did not classify the widget")
need(row["solved"] is True, "the widget refused the click: the page was not read first")
need(row["via"] == "field", "success was not observed as a token write")
visit = row.get("visit") or {}
need(visit.get("ok") is True, "the prelude did not run in the top document")
need(int(visit.get("moves", 0)) >= 6, "the pointer barely crossed the page")
need(int(visit.get("wheels", 0)) >= 1, "the page was never scrolled")
print(
    f"ok: the page was read first ({visit['moves']} moves, {visit['wheels']} wheels "
    f"in {visit['ms']}ms), then the widget accepted the click, token via {row['via']} "
    f"in {row['ms']}ms"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

# Phase 2: the same page and the same click with no visit at all. An empty prelude
# survives the driver's fill-in, so this is the solve every other tool performs.
bare="$work/bare.jsonl"
python3 - "$work/config.json" "$work/bare-config.json" "$bare" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
config["prelude"] = {"settle_ms": 0, "scroll": [], "wander": [], "dwell_ms": 0}
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/bare-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$bare" ]] || { echo "FAIL: the engine wrote no evidence for the unread page"; exit 1; }
python3 - "$bare" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
row = [r for r in rows if "solved" in r][-1]
if row["solved"] is not False:
    print(f"FAIL: the widget accepted a click on a page nobody read: {row}")
    sys.exit(1)
visit = row.get("visit") or {}
if visit.get("moves"):
    print(f"FAIL: an empty prelude still moved the pointer: {row}")
    sys.exit(1)
print(f"ok: a click with no visit was refused: {row['error']}")
PY

echo "PASS: the engine read the page before it acted, and was refused when it did not"

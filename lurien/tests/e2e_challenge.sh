#!/usr/bin/env bash
# The engine-level solver, end to end, against a widget in its own browsing
# context.
#
# The fixture writes its token only for a trusted click that arrived after real
# pointer motion, and it lives on a different host than the page that embeds it,
# so it gets its own context and its own process. A written token therefore
# proves the whole chain: the observer attached to a cross-origin child context,
# the catalog target was located inside that context, the pointer approached and
# pressed through the real event path, and the token was observed rather than
# assumed.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_challenge.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# The host owns the target directory; ask cargo rather than hardcoding a path.
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
# A free port, so a stale server from another run cannot answer for the fixture.
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }

# The page is served from localhost, the widget from 127.0.0.1. Same server, two
# hosts, so the widget is cross-origin and gets its own context.
mkdir -p "$work/www"
sed "s|CHILD_URL|http://127.0.0.1:$port/challenge_checkbox_child.html|" \
  "$fixtures/challenge_checkbox_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_checkbox_child.html" "$work/www/"
( cd "$work/www" && python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# One binding, addressed by kind. The engine learns the vendor from data, which
# is the whole point: this fixture is a vendor as far as the engine can tell.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_challenge.sh",
      "target": "first checkbox in this BC",
      "iframe_src": ["challenge_checkbox_child.html"],
      "custom_elements": [],
      "selectors": [],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 20000,
  "claimed_kinds": ["none", "score", "checkbox", "fail"],
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
row = rows[-1]
def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)
need(row["kind"] == "checkbox", "the engine did not classify the widget as a checkbox")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the fixture refused the click, so it was not a trusted one after real motion")
need(row["via"] == "field", "success was not observed as a token write")
need(row["contexts"] >= 2, "the observer did not attach to both the page and the widget context")
need(row["source"] == "engine", "the solve did not come from the engine")
need(row["ms"] > 0, "the solve reported no elapsed time")
print(f"ok: {row['kind']} solved via {row['via']} in {row['ms']}ms across {row['contexts']} contexts")
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"checkbox"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

echo "PASS: the engine cleared a checkbox challenge in the widget's own browsing context"

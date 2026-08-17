#!/usr/bin/env bash
# The browser says it started, and a browser that does not is refused.
#
# Every other proof in this directory shows the subsystem doing work. This one
# shows what happens when it does no work at all, because that is the failure the
# page probe cannot see: the probe reads the top document, so a browser with no
# observer in it answers `none` for every guarded page on the internet, and `none`
# is also the honest answer for a clean page. A caller acting on the first one
# walks into a challenge it was told was not there.
#
#   1. A clean page. The engine writes one `started` row before it has seen a
#      page, and that row names the number of catalog bindings the caller shipped.
#      `goto` succeeds and reports `none`.
#   2. A session whose evidence cannot be written: the path names a directory that
#      does not exist, so the row never lands. The page is clean, the probe would
#      say `none`, and `goto` must refuse with the missing start instead.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_bootstrap.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
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
cat > "$work/www/clean.html" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>a page with nothing on it</title>
<p id="body">No widget here.</p>
HTML
( cd "$work/www" && exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  curl -fsS "http://127.0.0.1:$port/clean.html" >/dev/null 2>&1 && break
  sleep 0.25
done
curl -fsS "http://127.0.0.1:$port/clean.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# Two bindings that match nothing on this page. The count is the claim: the start
# row reports what the observer loaded, so a catalog that arrived truncated is a
# different number rather than a silent one.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "one",
      "kind": "checkbox",
      "source": "e2e_bootstrap.sh",
      "target": "first checkbox in this BC",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["#never-here"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    },
    {
      "name": "two",
      "kind": "slider",
      "source": "e2e_bootstrap.sh",
      "target": "first canvas in this BC",
      "handle": "first draggable in this BC",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["#never-here-either"],
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

# Phase 1: the browser announces its start, and a clean page reads as clean.
set +e
out="$(LURIEN_CHALLENGE="$(cat "$work/config.json")" "$lurien" --headless goto "http://127.0.0.1:$port/clean.html" 2>&1)"
status=$?
set -e
echo "goto: $out"
[[ $status -eq 0 ]] || { echo "FAIL: a clean page was refused: $out"; exit 1; }
[[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence at all, so it never started"; exit 1; }
echo "evidence: $(cat "$evidence")"

python3 - "$evidence" <<'PY'
import json, sys

rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
starts = [r for r in rows if r.get("event") == "started"]
if len(starts) != 1:
    print(f"FAIL: the engine wrote {len(starts)} start rows, expected exactly one per session")
    sys.exit(1)
row = starts[0]
if rows[0] is not row:
    print("FAIL: the start row is not the first row, so it was written after a page was seen")
    sys.exit(1)
if row.get("v") != 1:
    print(f"FAIL: the start row is stamped {row.get('v')!r}, which no driver reads")
    sys.exit(1)
if row.get("bindings") != 2:
    print(f"FAIL: the start row names {row.get('bindings')!r} bindings, the caller shipped 2")
    sys.exit(1)
print(f"start: {row}")
PY

case "$out" in
  *'"kind":"none"'*|*'"kind": "none"'*) ;;
  *) echo "FAIL: a page with no widget was not reported as none: $out"; exit 1;;
esac

# Phase 2: a session whose start row cannot land. The page is the same clean page,
# so the only thing that changed is the driver's proof that anything was watching.
# The directory is read-only, which the engine's own writer creates a path under
# when it can: a missing directory is not blindness, it is a directory the browser
# makes for itself.
mkdir -p "$work/blind"
chmod 500 "$work/blind"
missing="$work/blind/evidence.jsonl"
python3 - "$work/config.json" "$work/blind.json" "$missing" <<'PY'
import json, sys

config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
json.dump(config, open(sys.argv[2], "w"))
PY

set +e
blind="$(LURIEN_CHALLENGE="$(cat "$work/blind.json")" "$lurien" --headless goto "http://127.0.0.1:$port/clean.html" 2>&1)"
blind_status=$?
set -e
echo "blind: $blind"
chmod 700 "$work/blind"
[[ -e "$missing" ]] && { echo "FAIL: the evidence row landed after all, so this phase proves nothing"; exit 1; }
[[ $blind_status -ne 0 ]] || { echo "FAIL: a browser that never said it started was trusted: $blind"; exit 1; }
case "$blind" in
  *"never started its challenge subsystem"*) ;;
  *) echo "FAIL: the refusal did not name the missing start: $blind"; exit 1;;
esac
case "$blind" in
  *"$missing"*) ;;
  *) echo "FAIL: the refusal did not name the evidence path it read: $blind"; exit 1;;
esac

echo "PASS: the browser announced its start on a clean page, and a session with no start was refused"

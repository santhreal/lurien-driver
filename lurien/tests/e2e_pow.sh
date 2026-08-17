#!/usr/bin/env bash
# The pow kind, end to end, against a page that mints a fresh hash target.
#
# The fixture picks a random challenge and a random difficulty on every load, and
# writes its token only for an answer whose digest clears that difficulty, typed
# key by key with trusted events. So a written token proves the whole chain: the
# challenge was read out of the page's own global, the search ran in the browser
# and found a real nonce, and the answer was entered through the keyboard path
# rather than assigned. No helper process and no service is involved.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_pow.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# The host owns the target directory; ask cargo rather than hardcoding a path.
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
cp "$fixtures/challenge_pow.html" "$work/www/"
# `exec` so `$!` is python itself, not the subshell that would leave it running.
( cd "$work/www" && exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://127.0.0.1:$port/challenge_pow.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://127.0.0.1:$port/challenge_pow.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }

# One binding. The work table is the whole vendor description: where the
# challenge lives, how the difficulty is counted, and where the answer goes.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "pow",
      "source": "e2e_pow.sh",
      "target": "input[name=pow-response]",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["input[name=pow-response]"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": [],
      "work": {
        "algo": "sha256",
        "format": "hex-zeros",
        "challenge": "global:__pow.challenge",
        "difficulty": "global:__pow.difficulty",
        "submit": "field:input[name=pow-response]"
      }
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 30000,
  "claimed_kinds": ["none", "score", "checkbox", "pow", "fail"],
  "poll_ms": 200
}
JSON

export LURIEN_CHALLENGE="$(cat "$work/config.json")"
set +e
out="$("$lurien" --headless goto "http://127.0.0.1:$port/challenge_pow.html" 2>&1)"
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
need(row["kind"] == "pow", "the engine did not classify the page as pow")
need(row["vendor"] == "fixture", "the engine did not name the binding that matched")
need(row["solved"] is True, "the fixture refused the answer, so it was wrong or was not typed")
need(row["via"] == "field", "success was not observed as a token write")
need(row["source"] == "engine", "the solve did not come from the engine")
need(row["ms"] > 0, "the solve reported no elapsed time")
work = row.get("work") or {}
need(work.get("submitted") == "field", "the answer did not go in through the keyboard path")
need(int(work.get("difficulty", 0)) >= 3, "the fixture asked for no real work")
# The count is a random variable: a lucky first nonce is a real solve, so this
# asserts the engine hashed at all. That the search is correct rather than lucky
# is pinned by lurien/tests/pow_sha256.mjs, which checks the digest and the
# difficulty predicate against a reference implementation.
need(int(work.get("tried", 0)) >= 1, "the engine reported no hashes tried")
need(int(work.get("lanes", 0)) >= 1, "the engine reported no grinding lanes")
print(
    f"ok: pow cleared {work['difficulty']} hex zeros in {work['tried']} hashes "
    f"across {work['lanes']} lanes, token via {row['via']} in {row['ms']}ms"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"pow"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

# The other half of the claim: a binding whose challenge address resolves to
# nothing must be refused, not reported as a pass. Same page, same fixture, one
# wrong address.
wrong="$work/wrong.jsonl"
sed -e "s|global:__pow.challenge|global:__pow.nosuchfield|" \
    -e "s|$evidence|$wrong|" "$work/config.json" > "$work/wrong-config.json"
LURIEN_CHALLENGE="$(cat "$work/wrong-config.json")" \
  "$lurien" --headless goto "http://127.0.0.1:$port/challenge_pow.html" >/dev/null 2>&1 || true
[[ -s "$wrong" ]] || { echo "FAIL: the engine wrote no evidence for the misaddressed binding"; exit 1; }
python3 - "$wrong" <<'PY'
import json, sys
row = [json.loads(line) for line in open(sys.argv[1]) if line.strip()][-1]
if row["solved"] is not False:
    print(f"FAIL: a binding that could not read its challenge reported a solve: {row}")
    sys.exit(1)
if "nosuchfield" not in (row["error"] or ""):
    print(f"FAIL: the refusal does not name the address that resolved to nothing: {row}")
    sys.exit(1)
print(f"ok: refused with {row['error']}")
PY

echo "PASS: the engine computed and typed a proof of work the page accepted"

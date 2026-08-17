#!/usr/bin/env bash
# Pointer dynamics per interaction, against a widget in its own browsing context.
#
# Every other solver, and every earlier build of this one, samples one path when
# the session starts and replays it for every widget it touches. A vendor scoring
# two widgets on one page, or nine cells of one grid, sees the same curve twice,
# which is a stronger signal than a bad curve: no hand repeats itself.
#
# The driver ships a deck of sampled paths and the seed it drew them with; the
# engine deals one per interaction and records which. Four claims:
#
#   1. Two solves in one session took two different entries of the deck. The
#      evidence row for each visit names the entry, so the log can be checked.
#   2. Both were real approaches through the trusted event path: the fixture
#      refuses a click that arrived with fewer than three trusted moves, and it
#      wrote its token on every visit.
#   3. The order is reproducible: a second session with the same
#      LURIEN_DYNAMICS_SEED deals the same entries in the same order.
#   4. The widget saw pointer motion it can measure, in its own coordinates.
#
# The dealt entry is the claim, not the coordinates the page recorded: the event
# path coalesces mousemove, so a page observes a different sample of one curve
# every time, and comparing samples would call one repeated path two shapes.
#
# That the deck entries are distinct shapes, and that one seed redraws the whole
# deck, is a unit test in `lurien/src/challenge.rs`.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_dynamics.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
fixtures="$root/captcha/kinds/fixtures"
port="${LURIEN_SERVE_PORT:-7493}"
base="http://127.0.0.1:$port"
work="$(mktemp -d)"
evidence="$work/evidence.jsonl"
seed=20260816
failed=0

if [ -z "$engine" ] || [ ! -x "$engine" ]; then
  echo "SKIP: set LURIEN_BIN to the engine binary"
  exit 0
fi
if [ ! -x "$serve" ]; then
  echo "SKIP: build the driver first (cargo build -p lurien-driver)"
  exit 0
fi

cleanup() {
  [ -n "${serve_pid:-}" ] && kill "$serve_pid" 2>/dev/null
  [ -n "${server_pid:-}" ] && kill "$server_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; failed=1; }

cmd() {
  local command="$1" extra="${2:-}" context="${3:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$context\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  curl -s --max-time 180 -H 'Content-Type: application/json' -d "$body}" "$base/v1/browser/command"
}

# The handle of the frame whose url contains a substring, out of `frames`.
handle_of() {
  printf '%s' "$1" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
data = json.loads(reply.get("output", "{}") or "{}")
for frame in data.get("frames", []):
    if sys.argv[1] in (frame.get("url") or ""):
        print(frame.get("handle") or "")
        break
' "$2"
}

# What a verb returned, as one line.
plain() {
  printf '%s' "$1" | python3 -c '
import json, sys
reply = json.load(sys.stdin)
out = reply.get("output", "")
if isinstance(out, str):
    print(out.strip())
else:
    print(json.dumps(out))
'
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
# The page on localhost, the widget on 127.0.0.1: one server, two origins, so the
# widget owns its own browsing context the way a real one does.
sed "s|CHILD_URL|http://127.0.0.1:$fixture_port/challenge_checkbox_child.html|" \
  "$fixtures/challenge_checkbox_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_checkbox_child.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!
url="http://localhost:$fixture_port/parent.html"
for _ in $(seq 1 40); do
  if curl -fsS "$url" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "$url" >/dev/null 2>&1 || { echo "FAIL: fixture server never came up"; exit 1; }

cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "fixture",
      "kind": "checkbox",
      "source": "e2e_dynamics.sh",
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

LURIEN_CHALLENGE="$(cat "$work/config.json")" LURIEN_DYNAMICS_SEED="$seed" \
  LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=60000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

# One visit: navigate, let the engine solve, and read what the widget saw.
visit() {
  local context="$1"
  cmd goto "\"args\":{\"url\":\"$url\"}" "$context" >/dev/null
  local frames child reply
  frames="$(cmd dom_frames "" "$context")"
  child="$(handle_of "$frames" challenge_checkbox_child.html)"
  if [ -z "$child" ]; then
    echo "FAIL: the widget frame has no handle: $frames" >&2
    return 1
  fi
  reply="$(cmd execute_js "\"args\":{\"code\":\"window.lurienApproach()\",\"frame\":\"$child\"}" "$context")"
  plain "$reply"
}

for pass in a b; do
  context="dyn-$$-$pass"
  reply="$(cmd launch "\"profile_dir\":\"$work/profile-$pass\",\"url\":\"about:blank\"" "$context")"
  echo "$reply" | grep -q '"success":true' \
    || { echo "FAIL: launch failed: $reply"; cat "$work/serve.log"; exit 1; }
  first="$(visit "$context")" || exit 1
  second="$(visit "$context")" || exit 1
  printf '%s\n' "$first" > "$work/approach-$pass-1.json"
  printf '%s\n' "$second" > "$work/approach-$pass-2.json"
  cmd close "" "$context" >/dev/null
done

python3 - "$work" "$evidence" <<'PY'
import json, sys

work, evidence = sys.argv[1], sys.argv[2]
failed = False


def bad(msg):
    global failed
    print(f"FAIL: {msg}")
    failed = True


def load(name):
    raw = open(f"{work}/{name}").read().strip()
    try:
        value = json.loads(raw)
    except Exception:
        bad(f"{name} is not a path the widget reported: {raw[:120]!r}")
        return []
    if isinstance(value, str):
        value = json.loads(value)
    return value


a1, a2 = load("approach-a-1.json"), load("approach-a-2.json")
b1, b2 = load("approach-b-1.json"), load("approach-b-2.json")

rows = [json.loads(line) for line in open(evidence) if line.strip()]
verdicts = [r for r in rows if "solved" in r]

# Claim 2: the fixture wrote its token on every visit, which it does only for a
# trusted click that arrived after at least three trusted moves.
solved = [r for r in verdicts if r.get("solved") is True and r.get("kind") == "checkbox"]
if len(solved) != 4:
    bad(
        f"{len(solved)} of 4 visits wrote the token: "
        + str([
            {"kind": r.get("kind"), "solved": r.get("solved"), "via": r.get("via"), "error": r.get("error")}
            for r in verdicts
        ])
    )
    sys.exit(1)

# Claim 4: the widget measured the motion in its own coordinates.
for name, path in (("a1", a1), ("a2", a2), ("b1", b1), ("b2", b2)):
    if len(path) < 3:
        bad(f"the widget recorded {len(path)} trusted moves for {name}, which is not an approach")

dealt = [r.get("dyn", {}).get("trajectory") for r in solved]
if any(index is None for index in dealt):
    bad(f"an evidence row does not name the deck entry it took: {dealt}")
    sys.exit(1)

# Claim 1: two interactions in one session did not take one entry.
if dealt[0] == dealt[1]:
    bad(f"both solves in one session took deck entry {dealt[0]}")
if dealt[2] == dealt[3]:
    bad(f"both solves in the replay session took deck entry {dealt[2]}")

# Claim 3: the same seed deals the same entries, in order.
if dealt[:2] != dealt[2:]:
    bad(f"the same seed dealt {dealt[:2]} and then {dealt[2:]}")

if failed:
    sys.exit(1)
print(f"ok: deck entries {dealt[:2]} replayed as {dealt[2:]}, {len(a1)} and {len(a2)} trusted moves recorded")
PY
status=$?

if [ "$failed" -ne 0 ] || [ "$status" -ne 0 ]; then
  echo "--- serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: two solves in one session took different entries of the sampled deck, both trusted, and the same seed dealt the same order"

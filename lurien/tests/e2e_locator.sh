#!/usr/bin/env bash
# The selector forms and the wait, end to end, over the HTTP face.
#
# Six claims:
#
#   1. `role:button=Log in` clicks the button a person would name.
#   2. `text:Continue` lands on the innermost element holding the text.
#   3. `label:Email` and `placeholder:you@example.com` reach their controls.
#   4. `testid:submit` reaches the element by its test id.
#   5. A button that does not exist yet is clicked without any explicit wait,
#      because the act waits for it. The fixture records when it appeared.
#   6. A description that fits two visible buttons is refused, naming both, and a
#      present-but-invisible or disabled element is refused rather than clicked.
#
# One browser for the whole run: this is the wire an agent runtime uses, and a
# per-form process launch would prove the same thing eight times as slowly.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_locator.sh
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)}"
serve="${LURIEN_SERVE:-$target/debug/lurien}"
engine="${LURIEN_BIN:-}"
port="${LURIEN_SERVE_PORT:-7481}"
base="http://127.0.0.1:$port"
ctx="locator-$$"
work="$(mktemp -d)"
failed=0

if [ -z "$engine" ] || [ ! -x "$engine" ]; then
  echo "SKIP: LURIEN_BIN unset or not executable"
  exit 0
fi
if [ ! -x "$serve" ]; then
  echo "SKIP: $serve not built (cargo build -p lurien-driver)"
  exit 0
fi

cleanup() {
  [ -n "${serve_pid:-}" ] && kill "$serve_pid" 2>/dev/null
  [ -n "${server_pid:-}" ] && kill "$server_pid" 2>/dev/null
  rm -rf "$work"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; failed=1; }

post() {
  curl -s --max-time 120 -H 'Content-Type: application/json' -d "$1" "$base/v1/browser/command"
}

cmd() {
  local command="$1" extra="${2:-}"
  local body="{\"schema_version\":1,\"backend\":\"guise_foxdriver\",\"command\":\"$command\",\"browser_context_id\":\"$ctx\",\"role\":\"e2e\",\"profile_id\":\"e2e\""
  if [ -n "$extra" ]; then body="$body,$extra"; fi
  post "$body}"
}

click() {
  cmd dom_click "\"args\":{\"selector\":\"$1\"}"
}

fixture_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
mkdir -p "$work/www"
cp "$root/captcha/kinds/fixtures/locator_forms.html" "$work/www/index.html"
( cd "$work/www" && exec python3 -m http.server "$fixture_port" --bind 127.0.0.1 >/dev/null 2>&1 ) &
server_pid=$!
for _ in $(seq 1 40); do
  curl -fsS "http://127.0.0.1:$fixture_port/index.html" >/dev/null 2>&1 && break
  sleep 0.25
done

# A short deadline keeps the refusals quick and still covers the late element,
# which the fixture adds 1.5 s after load.
LURIEN_BIN="$engine" LURIEN_SERVE_BIND="127.0.0.1:$port" LURIEN_TIMEOUT_MS=4000 \
  MOZ_DISABLE_CONTENT_SANDBOX=1 "$serve" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
curl -s --retry 40 --retry-delay 1 --retry-connrefused --max-time 5 "$base/v1/health" -o /dev/null \
  || { echo "FAIL: lurien serve did not start"; cat "$work/serve.log"; exit 1; }

out="$(cmd launch "\"profile_dir\":\"$work/profile\",\"url\":\"about:blank\"")"
echo "$out" | grep -q '"success":true' || { echo "FAIL: launch failed: $out"; exit 1; }
out="$(cmd goto "\"url\":\"http://127.0.0.1:$fixture_port/index.html\"")"
echo "$out" | grep -q '"success":true' || { echo "FAIL: goto failed: $out"; exit 1; }

for selector in "role:button=Log in" "text:Continue to checkout" "label:Email" \
                "placeholder:you@example.com" "testid:submit" "role:button=Publish"; do
  out="$(click "$selector")"
  echo "$out" | grep -q '"success":true' || fail "$selector was not clicked: $out"
done

# The refusals. Each must name what went wrong, not merely fail.
out="$(click "role:button=Send")"
echo "$out" | grep -q '"success":false' || fail "an ambiguous description was accepted: $out"
echo "$out" | grep -q '2 visible elements' || fail "the ambiguity was not explained: $out"
out="$(click "role:button=Ghost")"
echo "$out" | grep -q '"success":false' || fail "an invisible button was clicked: $out"
echo "$out" | grep -q 'none is visible' || fail "the invisible button was not explained: $out"
out="$(click "role:button=Archive")"
echo "$out" | grep -q '"success":false' || fail "a disabled button was clicked: $out"
out="$(click "role:button=Nothing Here")"
echo "$out" | grep -q 'on screen now' || fail "a missing element was not answered with what is: $out"

log="$(cmd execute_js "\"args\":{\"code\":\"JSON.stringify(window.__log)\"}")"
printf '%s' "$log" > "$work/log.json"
python3 - "$work/log.json" <<'PY'
import json, sys
reply = json.load(open(sys.argv[1]))

def rows(value):
    """The log is a JSON string somewhere in the reply; find it, whatever wraps it."""
    if isinstance(value, str):
        if '"what"' in value:
            try:
                return json.loads(value)
            except json.JSONDecodeError:
                return None
        return None
    if isinstance(value, dict):
        for item in value.values():
            found = rows(item)
            if found is not None:
                return found
    if isinstance(value, list):
        for item in value:
            found = rows(item)
            if found is not None:
                return found
    return None

log = rows(reply)
if log is None:
    print(f"FAIL: the page log was unreadable: {json.dumps(reply)[:400]}")
    sys.exit(1)
what = [row["what"] for row in log]
def need(item, why):
    if item not in what:
        print(f"FAIL: {why}: {what}")
        sys.exit(1)
need("a1:clicked", "role: did not click the named button")
need("a3:clicked", "text: did not land on the innermost element")
need("a4:focused", "label: did not reach the labelled field")
need("a5:focused", "placeholder: did not reach the field")
need("submit:clicked", "testid: did not reach the element")
need("a11:clicked", "the late button was never clicked")
need("late:appeared", "the fixture never added the late button")
if "untrusted" in what:
    print(f"FAIL: an act arrived as an untrusted event: {what}")
    sys.exit(1)
for row in log:
    if row["what"] == "a11:clicked" and row["atMs"] < 1500:
        print(f"FAIL: the late button was clicked before it existed: {row}")
        sys.exit(1)
for row in log:
    if row["what"] in ("a7:clicked", "a8:clicked", "a9:clicked", "a10:clicked"):
        print(f"FAIL: a refused selector still acted: {row}")
        sys.exit(1)
appeared = next(r["atMs"] for r in log if r["what"] == "late:appeared")
clicked = next(r["atMs"] for r in log if r["what"] == "a11:clicked")
print(f"ok: every form resolved; the late button appeared at {appeared}ms and was clicked at {clicked}ms")
PY
[ $? -eq 0 ] || failed=1

cmd close >/dev/null

if [ "$failed" -ne 0 ]; then
  echo "--- lurien serve log ---"
  tail -40 "$work/serve.log"
  exit 1
fi
echo "PASS: role, text, label, placeholder and testid resolve, the wait covers a late element, and an ambiguous description is refused"

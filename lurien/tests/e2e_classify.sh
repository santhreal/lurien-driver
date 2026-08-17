#!/usr/bin/env bash
# Two challenges on one page, end to end.
#
# A page can hold more than one widget, and the cheap one is not the one that
# gates it. This fixture holds a checkbox and a slider, both cross-origin. In
# both phases the checkbox binding matches two signals and the slider binding
# one, so signal count and severity disagree and only one of them picks the
# widget that leaves the page unusable when it is skipped.
#
# Ordering happens in two places, and each phase pins one of them by giving the
# other nothing to decide:
#
#   1. Reduction across contexts. Each binding matches only inside its own
#      widget frame, so every context holds one candidate and the choice is made
#      when the contexts are merged. The verdict must be `slider`, the drag must
#      have happened, and the report must name both widgets.
#   2. Classification inside one context. Both bindings match in the top
#      document alone, so there is nothing to merge and the choice is made while
#      the signals are read. That context's own kind must be `slider`.
#
# Usage: LURIEN_BIN=/path/to/engine bash lurien/tests/e2e_classify.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$(cd "$root" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')}"
lurien="${LURIEN_CLI:-$target/debug/lurien}"
vision="${LURIEN_VISION:-$target/debug/lurien-vision}"
fixtures="$root/captcha/kinds/fixtures"
work="$(mktemp -d)"
port="${FIXTURE_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}"
helper_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
helper_token="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
evidence="$work/evidence.jsonl"

cleanup() {
  [[ -n "${server_pid:-}" ]] && kill "$server_pid" 2>/dev/null || true
  [[ -n "${helper_pid:-}" ]] && kill "$helper_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$lurien" ]] || { echo "FAIL: no lurien binary at $lurien"; exit 1; }
[[ -x "$vision" ]] || { echo "FAIL: no vision helper at $vision (cargo build -p lurien-vision)"; exit 1; }

mkdir -p "$work/www"
# The page is served from localhost and both widgets from 127.0.0.1: same server,
# two hosts, so each widget is cross-origin and gets its own context and process.
sed -e "s|SLIDER_URL|http://127.0.0.1:$port/challenge_slider.html|" \
    -e "s|CHECKBOX_URL|http://127.0.0.1:$port/challenge_checkbox_child.html|" \
  "$fixtures/challenge_two_widgets_parent.html" > "$work/www/parent.html"
cp "$fixtures/challenge_slider.html" "$fixtures/challenge_checkbox_child.html" "$work/www/"
( cd "$work/www" && exec python3 -m http.server "$port" --bind 0.0.0.0 >/dev/null 2>&1 ) &
server_pid=$!

"$vision" --port "$helper_port" --token "$helper_token" > "$work/helper.log" 2>&1 &
helper_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "http://localhost:$port/parent.html" >/dev/null 2>&1; then break; fi
  sleep 0.25
done
curl -fsS "http://localhost:$port/parent.html" >/dev/null || { echo "FAIL: fixture server never came up"; exit 1; }
for _ in $(seq 1 40); do
  if python3 -c "
import socket, sys
s = socket.socket()
s.settimeout(0.3)
sys.exit(0 if s.connect_ex(('127.0.0.1', $helper_port)) == 0 else 1)
"; then break; fi
  sleep 0.25
done

# Phase 1: reduction across contexts. Each binding names elements that exist only
# inside its own widget frame, so the top document matches nothing, every context
# holds exactly one candidate, and the kind of the page is decided when the
# contexts are merged. The checkbox frame satisfies two of its binding's
# selectors and the slider frame one of its, so a merge by signal count reports
# the checkbox.
cat > "$work/config.json" <<JSON
{
  "catalog": [
    {
      "name": "cheap",
      "kind": "checkbox",
      "source": "e2e_classify.sh",
      "target": "first checkbox in this BC",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["#box", "#label"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    },
    {
      "name": "gate",
      "kind": "slider",
      "source": "e2e_classify.sh",
      "target": "first canvas in this BC",
      "handle": "first draggable in this BC",
      "iframe_src": [],
      "custom_elements": [],
      "selectors": ["#puzzle"],
      "cookies": [],
      "scripts": [],
      "token_inputs": ["fixture-token"],
      "token_cookies": []
    }
  ],
  "evidence": "$evidence",
  "budget_ms": 20000,
  "claimed_kinds": ["none", "score", "checkbox", "pow", "slider", "fail"],
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

[[ -s "$evidence" ]] || { echo "FAIL: the engine wrote no evidence for the fixture page"; exit 1; }
echo "evidence: $(cat "$evidence")"

python3 - "$evidence" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
verdicts = [r for r in rows if "solved" in r]
if not verdicts:
    print(f"FAIL: the engine reported no verdict for the page: {rows}")
    sys.exit(1)
row = verdicts[-1]


def need(cond, msg):
    if not cond:
        print(f"FAIL: {msg}: {row}")
        sys.exit(1)


# Claim 1: severity, not signal count. The checkbox binding matched two signals
# in the parent and the slider one; a reduce by count reports `checkbox` here.
need(row["kind"] == "slider", "the page was classified by signal count, not by what gates it")
need(row["vendor"] == "gate", "the engine named the wrong binding for the kind it acted on")
need(row["solved"] is True, "the puzzle was not solved")
need(row["via"] == "field", "success was not observed as a token write")
work = row.get("work") or {}
need(float(work.get("dx", 0)) > 40, "the verdict says slider but nothing was dragged")
need(int(work.get("moves", 0)) >= 8, "the drag was dispatched as too few moves to be a hand")

# Claim 2: both widgets are named, most severe first, each vendor once.
seen = row.get("seen")
need(isinstance(seen, list), "the row names no widgets at all")
pairs = [(entry.get("kind"), entry.get("vendor")) for entry in seen]
need(("slider", "gate") in pairs, f"the widget that gates the page is missing from seen: {pairs}")
need(("checkbox", "cheap") in pairs, f"the widget that was passed over is missing from seen: {pairs}")
need(len(pairs) == len(set(pairs)), f"a widget is named twice: {pairs}")
need(pairs[0] == ("slider", "gate"), f"seen is not ordered most severe first: {pairs}")
counts = {entry.get("kind"): entry.get("signals") for entry in seen}
need(
    counts.get("checkbox", 0) > counts.get("slider", 0),
    f"the fixture no longer makes count and severity disagree: {counts}",
)
print(
    f"ok: {len(pairs)} widgets seen {pairs}, the page was gated by "
    f"{row['kind']} and solved by dragging {work['dx']:.1f}px in {work['moves']} moves"
)
PY

if [[ $status -ne 0 ]]; then
  echo "FAIL: goto exited $status after the engine reported a solve"
  exit 1
fi

case "$out" in
  *'"kind":"slider"'*) ;;
  *) echo "FAIL: goto did not report the engine's kind: $out"; exit 1 ;;
esac

# The face reports the same list the evidence does: a caller reading the verb
# output must be able to see the widget the engine did not act on.
python3 - "$out" <<'PY'
import json, sys
# The binary may have written diagnostics to the same stream, so the report is
# the last line that parses as an object.
report = None
for line in sys.argv[1].splitlines():
    line = line.strip()
    if line.startswith("{"):
        try:
            report = json.loads(line)
        except json.JSONDecodeError:
            pass
if report is None:
    print(f"FAIL: goto printed no json report: {sys.argv[1]}")
    sys.exit(1)
seen = {(entry["kind"], entry["vendor"]) for entry in report.get("seen", [])}
if seen != {("slider", "gate"), ("checkbox", "cheap")}:
    print(f"FAIL: goto did not report both widgets: {report}")
    sys.exit(1)
print(f"ok: goto reported both widgets and acted on {report['kind']}")
PY

# Phase 2: classification inside one context. Both bindings now name elements of
# the top document only, so one context holds both candidates and no merge can
# fix a wrong choice. That context's own kind is the claim, and the observer
# writes it per sighting when it is asked to say what it saw.
debug_evidence="$work/debug.jsonl"
python3 - "$work/config.json" "$work/parent-config.json" "$debug_evidence" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
config["evidence"] = sys.argv[3]
config["debug"] = True
for binding in config["catalog"]:
    # The checkbox binding matches the form and the frame that holds it, the
    # slider binding only the frame: two signals against one, in one document.
    binding["selectors"] = (
        ["#email", "#checkbox-frame"] if binding["kind"] == "checkbox" else ["#slider-frame"]
    )
json.dump(config, open(sys.argv[2], "w"))
PY
LURIEN_CHALLENGE="$(cat "$work/parent-config.json")" \
  "$lurien" --headless goto "http://localhost:$port/parent.html" >/dev/null 2>&1 || true
[[ -s "$debug_evidence" ]] || { echo "FAIL: the engine reported no sighting for the top document"; exit 1; }
python3 - "$debug_evidence" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
tops = [r for r in rows if r.get("event") == "sighting" and r.get("isTop")]
if not tops:
    print(f"FAIL: the observer never reported the top document: {rows}")
    sys.exit(1)
row = tops[-1]
if row["kind"] != "slider":
    print(f"FAIL: one context with two candidates was classified by signal count: {row}")
    sys.exit(1)
if row["vendor"] != "gate":
    print(f"FAIL: the kind was right and the binding was not: {row}")
    sys.exit(1)
print(f"ok: one context holding both widgets was classified {row['kind']} on {row['signals']}")
PY

echo "PASS: a page with two challenges was reduced to the one that gates it, in the context and across them, and both were reported"

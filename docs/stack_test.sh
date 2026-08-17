#!/usr/bin/env bash
#
# stack_test.sh: X065: the stack meta-command must be reproducible and LOUD on
# any host. These checks drive only the deterministic, infra-free seams of
# stack.sh (--help, --dry-run, the X063 target-dir guard, arg handling), so they
# pass identically on a fresh fleet host with no engine/DISPLAY/network.
#
# Run: ./stack_test.sh   (exit 0 = all assertions held)
#
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
STACK="$HERE/stack.sh"
fails=0
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1" >&2; fails=$((fails+1)); }

assert_contains() { # haystack needle label
  if [[ "$1" == *"$2"* ]]; then pass "$3"; else fail "$3 (missing: $2)"; fi
}
assert_exit() { # actual expected label
  if [[ "$1" == "$2" ]]; then pass "$3"; else fail "$3 (exit $1, want $2)"; fi
}

echo "== stack.sh contract tests =="

# 1. --help is documented and exits 0.
out="$("$STACK" --help 2>&1)"; rc=$?
assert_exit "$rc" 0 "--help exits 0"
assert_contains "$out" "lurien meta-command" "--help prints purpose"
assert_contains "$out" "X061" "--help documents skip-loud (X061)"
assert_contains "$out" "X063" "--help documents target-dir rule (X063)"

# 2. --dry-run exits 0 and names every stage (engine, guise, lurien, bench, scorecard).
out="$("$STACK" --dry-run 2>&1)"; rc=$?
assert_exit "$rc" 0 "--dry-run exits 0"
for stage in "engine:" "guise:build" "lurien:build" "bench:real-waf" "scorecard"; do
  assert_contains "$out" "$stage" "--dry-run includes stage $stage"
done
assert_contains "$out" "none silently passed" "--dry-run asserts no silent pass"

# 3. Every SKIP line carries a non-empty reason (X061 (never silently passed)).
#    Scan the rendered "SKIP:" lines; each must have text after the colon.
bad_skips=0
while IFS= read -r line; do
  reason="${line#*SKIP: }"
  [[ -z "${reason// /}" ]] && bad_skips=$((bad_skips+1))
done < <(printf '%s\n' "$out" | grep -F "SKIP:")
assert_exit "$bad_skips" 0 "every SKIP carries a reason"

# 4. X063 guard: a target dir INSIDE the tree must be a hard FATAL (exit 3).
intree="$(cd "$HERE/../.." >/dev/null 2>&1 && pwd)/target"
out="$(CARGO_TARGET_DIR="$intree" "$STACK" --dry-run 2>&1)"; rc=$?
assert_exit "$rc" 3 "in-tree CARGO_TARGET_DIR is FATAL"
assert_contains "$out" "X063" "in-tree target dir names X063"

# 5. Unknown argument is rejected (exit 2), not silently ignored.
out="$("$STACK" --frobnicate 2>&1)"; rc=$?
assert_exit "$rc" 2 "unknown arg exits 2"
assert_contains "$out" "unknown argument" "unknown arg explains itself"

echo "== done: $fails failure(s) =="
[[ "$fails" == 0 ]] || exit 1

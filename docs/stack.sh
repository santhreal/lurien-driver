#!/usr/bin/env bash
#
# stack.sh: lurien meta-command.
#
# Drives the stack in order: lurien engine -> guise -> captcha (in-tree) ->
# stack bench -> scorecard. Each stage RUNS, or SKIPS LOUD with the reason when
# its infra is absent (X061), a stage is never silently passed over. Stages that
# need heavy/networked infra (the engine build, the live bench) are opt-in so the
# default invocation is cheap and safe to run on any host (X065).
#
# Cargo target dirs are NEVER overridden here: the fleet pins them OUT of the
# Santh tree via ~/.cargo/config.toml (X063); this script asserts that and refuses
# to run if a target dir would land inside the tree.
#
# Usage:
#   stack.sh [--build-engine] [--bench] [--no-scorecard] [--dry-run] [--help]
#
# Tier-A config (env, overridable):
#   STACK_SANTH_ROOT   Santh tree root          (default: derived from this script)
#   LURIEN_STAGING     lurien gecko/build tree  (default: <root>/software/browser/engine/camoufox-150.0.2-beta.25;
#                      REYNARD_STAGING is a one-release alias)
#   LURIEN_BIN / REYNARD_BIN / GUISE_REYNARD_BIN  explicit engine binary
#
set -uo pipefail

# ----- locate the tree (portable: derive from this script's location) ---------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
SANTH_ROOT="${STACK_SANTH_ROOT:-$(cd "$SCRIPT_DIR/../../.." >/dev/null 2>&1 && pwd)}"
LURIEN_STAGING="${LURIEN_STAGING:-${REYNARD_STAGING:-$SANTH_ROOT/software/browser/engine/camoufox-150.0.2-beta.25}}"
REYNARD_STAGING="$LURIEN_STAGING"

# ----- flags ------------------------------------------------------------------
BUILD_ENGINE=0
RUN_BENCH=0
DO_SCORECARD=1
DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --build-engine) BUILD_ENGINE=1 ;;
    --bench)        RUN_BENCH=1 ;;
    --no-scorecard) DO_SCORECARD=0 ;;
    --dry-run)      DRY_RUN=1 ;;
    -h|--help)
      sed -n '3,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "stack.sh: unknown argument '$arg' (try --help)" >&2; exit 2 ;;
  esac
done

# ----- pretty + status accounting --------------------------------------------
bold() { printf '\033[1m%s\033[0m\n' "$*"; }
hr()   { printf '%s\n' "------------------------------------------------------------"; }
declare -a STAGE_NAME STAGE_STATUS STAGE_DETAIL
record() { STAGE_NAME+=("$1"); STAGE_STATUS+=("$2"); STAGE_DETAIL+=("$3"); }

run_stage() { # name, command...
  local name="$1"; shift
  bold ">>> $name"
  if [[ "$DRY_RUN" == 1 ]]; then
    echo "    [dry-run] would run: $*"
    record "$name" "DRY" "would run: $*"
    return 0
  fi
  if "$@"; then
    record "$name" "OK" "$*"
    return 0
  else
    local rc=$?
    echo "    !! $name FAILED (exit $rc)" >&2
    record "$name" "FAIL" "exit $rc: $*"
    return "$rc"
  fi
}

skip_stage() { # name, reason
  bold ">>> $1"
  echo "    SKIP: $2"
  record "$1" "SKIP" "$2"
}

# ----- X063: cargo target dir must be OUT of the Santh tree --------------------
resolve_cargo_target() {
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    printf '%s' "$CARGO_TARGET_DIR"; return
  fi
  local cfg="$HOME/.cargo/config.toml"
  [[ -f "$cfg" ]] || cfg="$HOME/.cargo/config"
  if [[ -f "$cfg" ]]; then
    # first `target-dir = "..."` under [build]
    grep -E '^\s*target-dir\s*=' "$cfg" 2>/dev/null | head -1 \
      | sed -E 's/.*=\s*"([^"]*)".*/\1/'
  fi
}

bold "lurien meta-build"
echo "  santh root      : $SANTH_ROOT"
echo "  engine staging  : $LURIEN_STAGING"
CARGO_TARGET="$(resolve_cargo_target)"
echo "  cargo target dir: ${CARGO_TARGET:-<cargo default ./target>}"
if [[ -n "$CARGO_TARGET" && "$CARGO_TARGET" == "$SANTH_ROOT"* ]]; then
  echo "FATAL (X063): cargo target dir '$CARGO_TARGET' is INSIDE the Santh tree." >&2
  echo "  Point build.target-dir in ~/.cargo/config.toml outside the tree and retry." >&2
  exit 3
fi
if [[ -z "$CARGO_TARGET" ]]; then
  echo "WARNING (X063): no explicit cargo target dir; cargo will write ./target inside the tree." >&2
fi
hr

# ----- engine resolution (mirrors guise::browser::resolve_lurien_bin order) ---
resolve_engine() {
  local c
  for c in "${LURIEN_BIN:-}" "${REYNARD_BIN:-}" "${GUISE_REYNARD_BIN:-}" \
           "$HOME/.local/share/lurien/lurien" \
           "$HOME/.local/share/reynard/reynard" \
           "$HOME/.cache/lurien/lurien" \
           "$HOME/.cache/reynard/reynard" \
           "/opt/lurien/lurien" \
           "/opt/reynard/reynard"; do
    [[ -n "$c" && -x "$c" ]] && { printf '%s' "$c"; return 0; }
  done
  for c in "$LURIEN_STAGING"/obj-*/dist/bin/camoufox; do
    [[ -x "$c" ]] && { printf '%s' "$c"; return 0; }
  done
  return 1
}

# ===== Stage 1: lurien engine ===============================================
if [[ "$BUILD_ENGINE" == 1 ]]; then
  if [[ -d "$LURIEN_STAGING" ]]; then
    if [[ "$DRY_RUN" == 1 ]]; then
      skip_stage "engine:build" "dry-run (would: cd $LURIEN_STAGING && ./mach configure && ./mach build)"
    else
      bold ">>> engine:build (heavy: Firefox build)"
      ( cd "$LURIEN_STAGING" && ./mach configure && ./mach build )
      if [[ $? -eq 0 ]]; then record "engine:build" "OK" "mach build"; else record "engine:build" "FAIL" "mach build"; fi
    fi
  else
    skip_stage "engine:build" "LURIEN_STAGING not found at $LURIEN_STAGING (set LURIEN_STAGING)"
  fi
fi
if ENGINE_BIN="$(resolve_engine)"; then
  if [[ "$DRY_RUN" == 1 ]]; then
    record "engine:verify" "DRY" "resolved $ENGINE_BIN"
    bold ">>> engine:verify"; echo "    [dry-run] resolved engine: $ENGINE_BIN"
  else
    bold ">>> engine:verify"
    VER="$("$ENGINE_BIN" --version 2>/dev/null | head -1)"
    echo "    engine: $ENGINE_BIN"
    echo "    version: ${VER:-<no --version output>}"
    record "engine:verify" "OK" "${VER:-$ENGINE_BIN}"
  fi
else
  skip_stage "engine:verify" "lurien engine not installed. Run software/browser/install.sh or set LURIEN_BIN. Missing engine is fatal; there is no Firefox fallback."
fi
hr

# ===== Stage 2: guise =========================================================
run_stage "guise:build" \
  cargo build --manifest-path "$SANTH_ROOT/software/browser/guise/Cargo.toml" --features browser
hr

# ===== Stage 3: lurien ========================================================
run_stage "lurien:build" \
  cargo build --manifest-path "$SANTH_ROOT/software/browser/lurien/Cargo.toml"
hr

# ===== Stage 4: scorecard =====================================================
# captchaforge bench is retired. The lurien prove note is the v1 scorecard.
if [[ "$DO_SCORECARD" == 1 ]]; then
  run_stage "scorecard:local" \
    test -f "$SANTH_ROOT/software/browser/docs/bench-results/lurien-v1-prove.md"
else
  skip_stage "scorecard" "--no-scorecard"
fi
if [[ "$RUN_BENCH" == 1 ]]; then
  skip_stage "bench:real-waf" "captchaforge bench retired; lurien scorecard is software/browser/docs/bench-results/"
else
  skip_stage "bench:real-waf" "not requested (captchaforge bench is retired)"
fi
hr

# ===== summary ================================================================
bold "stack summary"
fail=0
for i in "${!STAGE_NAME[@]}"; do
  printf '  %-22s %-5s %s\n' "${STAGE_NAME[$i]}" "${STAGE_STATUS[$i]}" "${STAGE_DETAIL[$i]}"
  [[ "${STAGE_STATUS[$i]}" == "FAIL" ]] && fail=1
done
hr
if [[ "$fail" == 1 ]]; then
  echo "RESULT: FAILED, one or more required stages failed (see above)." >&2
  exit 1
fi
echo "RESULT: OK (skipped stages reported their reason above; none silently passed)."

#!/usr/bin/env bash
# Run lurien's HEADFUL browser harnesses on santhserver instead of the local
# desktop: keeping the workstation free. santhserver is headless, so the browser
# runs under a virtual X display (Xvfb via xvfb-run); the engine build was
# rsync'd to ~/lurien-150 on santhserver (a self-contained, symlink-dereferenced
# copy of the desktop's obj-*/dist/bin).
#
# Why the extra env vs. run-on-santhserver.sh:
#   - LURIEN_BIN           : the harnesses gate on this; points at the copied build.
#                            REYNARD_BIN is a one-release alias.
#   - LD_LIBRARY_PATH      : santhserver's SYSTEM libnss3 is 3.79; lurien's libxul
#                            needs NSS_3.117. The bundled NSS lives beside the binary,
#                            so the loader must search there FIRST or XPCOM won't load.
#   - MOZ_DISABLE_CONTENT_SANDBOX=1 : the box can't unshare a PID namespace
#                            (CanCreateUserNamespace EPERM), disable the content
#                            sandbox so launch doesn't abort.
#   - xvfb-run -a          : allocate a free virtual display and export DISPLAY.
#
# Prereqs (one-time, already done, re-run if the build changes):
#   rsync -rlptD -L --partial <desktop>/obj-*/dist/bin/ santhserver:lurien-150/
#   (the -L is REQUIRED: dist/bin has ~6900 symlinks into the obj tree, e.g.
#    dependentlibs.list; preserved as symlinks they dangle and XPCOM fails to load.)
#
# Usage:
#   run-browser-on-santhserver.sh [--test <name>] [extra cargo/test args...]
# Examples:
#   run-browser-on-santhserver.sh                              # lurien_live_detectors
#   run-browser-on-santhserver.sh --test lurien_live_cloudflare
#   run-browser-on-santhserver.sh --test lurien_gate          # also needs STEALTH_FIREFOX
set -uo pipefail

SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=15 -o StrictHostKeyChecking=accept-new
          -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
HOST=santhserver
REMOTE_TREE=/mnt/santh-desktop
REMOTE_TARGET=/var/santh-cargo-target
ENGINE_DIR='$HOME/lurien-150'   # expanded remotely

TEST=lurien_live_detectors
EXTRA=()
while [ $# -gt 0 ]; do
  case "$1" in
    --test) TEST="$2"; shift 2 ;;
    *) EXTRA+=("$1"); shift ;;
  esac
done

echo "== probing santhserver + lurien build =="
if ! timeout 25 ssh "${SSH_OPTS[@]}" "$HOST" "test -x $ENGINE_DIR/camoufox" 2>/dev/null; then
  echo "ERROR: $ENGINE_DIR/camoufox missing or not executable on $HOST." >&2
  echo "  Re-sync it: rsync -rlptD -L --partial <desktop>/obj-*/dist/bin/ $HOST:lurien-150/" >&2
  exit 2
fi

# The differential harnesses additionally need a stock Firefox for the diff:
# lurien_gate (JS-surface oracle) and tls_fingerprint (JA3/JA4/H2 == stock). Forward
# STEALTH_FIREFOX whenever the caller exported it, tests that don't consume it ignore
# it, and the ones that do switch from "record only" to "ENFORCE the == stock claim".
STEALTH_ENV=""
if [ -n "${STEALTH_FIREFOX:-}" ]; then
  STEALTH_ENV="STEALTH_FIREFOX='$STEALTH_FIREFOX'"
fi

echo "== running $TEST headful on $HOST (Xvfb) =="
timeout 900 ssh "${SSH_OPTS[@]}" "$HOST" "
  cd '$REMOTE_TREE' && \
  LURIEN_BIN=$ENGINE_DIR/camoufox REYNARD_BIN=$ENGINE_DIR/camoufox \
  LD_LIBRARY_PATH=$ENGINE_DIR \
  MOZ_DISABLE_CONTENT_SANDBOX=1 \
  CARGO_TARGET_DIR='$REMOTE_TARGET' \
  CC=gcc CXX=g++ HOST_CC=gcc HOST_CXX=g++ \
  $STEALTH_ENV \
  xvfb-run -a cargo test -p guise --features browser --test '$TEST' ${EXTRA[*]:-} -- --nocapture --test-threads=1 2>&1 \
  | grep -vE 'Sandbox: CanCreateUserNamespace|unshare\(CLONE'
" | tail -40
rc=${PIPESTATUS[0]}
echo "== remote browser-test exit: $rc =="
exit "$rc"

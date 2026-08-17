#!/usr/bin/env bash
# Offload lurien's CPU-heavy cargo work to santhserver, keeping the
# local (desktop) machine free. Headful browser harnesses (oracle/creepjs/tls)
# stay local: they need this box's DISPLAY=:1 + the lurien engine, but every
# compile and the non-browser test suites run remotely.
#
# Prereq: Tailscale SSH to santhserver approved (visit the login URL ssh prints).
# Usage:  software/browser/docs/run-on-santhserver.sh [cargo-test-args...]
set -uo pipefail

SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new
          -o ServerAliveInterval=10 -o ServerAliveCountMax=3)
HOST=santhserver
REMOTE_TARGET=/var/santh-cargo-target

echo "== probing santhserver SSH =="
if ! timeout 25 ssh "${SSH_OPTS[@]}" "$HOST" 'echo ok' >/dev/null 2>&1; then
  echo "ERROR: cannot SSH to $HOST non-interactively." >&2
  echo "  Tailscale SSH likely needs approval, run 'ssh $HOST true' locally and visit the URL it prints." >&2
  exit 2
fi

# Discover the Santh tree on santhserver (it mounts the desktop's NFS share).
echo "== locating Santh tree on $HOST =="
REMOTE_TREE=$(timeout 30 ssh "${SSH_OPTS[@]}" "$HOST" '
  for d in /mnt/santh-desktop \
           /mnt/santh-desktop/Santh \
           /mnt/santh-desktop/SanthData/Santh \
           /mnt/santh-desktop/mnt/shared/SanthData/Santh \
           /mnt/shared/SanthData/Santh ; do
    if [ -d "$d/software/browser/guise" ]; then echo "$d"; break; fi
  done')
if [ -z "${REMOTE_TREE:-}" ]; then
  echo "ERROR: could not find the Santh tree on $HOST. Inspect: ssh $HOST 'ls /mnt/santh-desktop'" >&2
  exit 3
fi
echo "tree: $REMOTE_TREE   target: $REMOTE_TARGET"

# Default job: guise non-browser lib suite + workspace check. Override via args.
ARGS=("$@")
if [ ${#ARGS[@]} -eq 0 ]; then
  ARGS=(test -p guise --features browser --lib)
fi

echo "== running: cargo ${ARGS[*]} =="
# The shared shell env exports CC="distcc gcc" (the desktop's distributed-build
# setup), but santhserver has no distcc → cc-rs fails. Pin plain compilers for the
# remote build so any C-dependency (cc-rs) compiles natively.
CC_OVERRIDE='CC=gcc CXX=g++ HOST_CC=gcc HOST_CXX=g++ CC_x86_64_unknown_linux_gnu=gcc CXX_x86_64_unknown_linux_gnu=g++'
timeout 2400 ssh "${SSH_OPTS[@]}" "$HOST" \
  "cd '$REMOTE_TREE' && $CC_OVERRIDE CARGO_TARGET_DIR='$REMOTE_TARGET' cargo ${ARGS[*]} 2>&1" \
  | tail -80
rc=${PIPESTATUS[0]}
echo "== remote cargo exit: $rc =="
exit "$rc"

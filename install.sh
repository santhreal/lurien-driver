#!/usr/bin/env bash
# Wire a local lurien engine build to the path the resolver looks for.
#
# v1 does not download Gecko. There is no hosted engine tarball yet.
# This script finds a built camoufox/lurien binary and symlinks it to
# ~/.local/share/lurien/lurien.
#
# Usage:
#   software/browser/install.sh [/path/to/camoufox]
#   LURIEN_BIN=/path/to/bin software/browser/install.sh
#
# If nothing is found: exit 1 with the build recipe.
set -euo pipefail

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  echo "ERROR: lurien v1 is Linux x86_64 only (this host is $(uname -s) $(uname -m))." >&2
  exit 1
fi

DEST_DIR="${HOME}/.local/share/lurien"
DEST="${DEST_DIR}/lurien"
BIN_DIR="${HOME}/.local/bin"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
SANTH_ROOT="$(cd "${SCRIPT_DIR}/../.." >/dev/null 2>&1 && pwd)"

BIN="${1:-${LURIEN_BIN:-${REYNARD_BIN:-${GUISE_REYNARD_BIN:-}}}}"

if [ -z "${BIN}" ]; then
  # LURIEN_STAGING names the build tree when it is not next to this script.
  for root in \
    "${SANTH_ROOT}/software/browser/engine" \
    ${LURIEN_STAGING:+"${LURIEN_STAGING}"} \
    /opt/lurien-staging \
    "${HOME}/lurien-staging"
  do
    [ -d "${root}" ] || continue
    cand="$(ls -t "${root}"/camoufox-*/obj-*/dist/bin/camoufox 2>/dev/null | head -1 || true)"
    if [ -n "${cand}" ]; then
      BIN="${cand}"
      break
    fi
    if [ -x "${root}/lurien" ]; then
      BIN="${root}/lurien"
      break
    fi
  done
fi

if [ -z "${BIN}" ]; then
  echo "ERROR: lurien engine not installed. Run install.sh or set LURIEN_BIN." >&2
  echo "Build the engine, then re-run this script with the binary path:" >&2
  echo "  cd ${SANTH_ROOT}/software/browser/engine && make dir && make build" >&2
  echo "  $0 ${SANTH_ROOT}/software/browser/engine/camoufox-*/obj-*/dist/bin/camoufox" >&2
  echo "There is no CDN download in v1." >&2
  exit 1
fi

if [ ! -x "${BIN}" ]; then
  echo "ERROR: not an executable lurien engine: ${BIN}" >&2
  echo "Check with: file ${BIN}" >&2
  exit 1
fi

mkdir -p "${DEST_DIR}"
ln -sfn "${BIN}" "${DEST}"
echo "lurien engine: ${DEST} -> ${BIN}"
if "${DEST}" --version >/dev/null 2>&1; then
  printf 'engine version: '
  "${DEST}" --version
else
  echo "engine version: (--version unavailable)"
fi

mkdir -p "${BIN_DIR}"
# Put CLI/MCP on PATH only when they already exist (this script does not build Rust).
for name in lurien lurien-mcp; do
  if command -v "${name}" >/dev/null 2>&1; then
    echo "${name}: $(command -v "${name}")"
  elif [ -x "${HOME}/.cargo/bin/${name}" ]; then
    ln -sfn "${HOME}/.cargo/bin/${name}" "${BIN_DIR}/${name}"
    echo "${name}: ${BIN_DIR}/${name} -> ${HOME}/.cargo/bin/${name}"
  else
    echo "${name}: not on PATH. After the crate builds: cargo install --path ${SCRIPT_DIR}/lurien"
  fi
done

echo "Done. Resolver order: LURIEN_BIN, then ${DEST}."
echo "Playwright: firefox.launch({ executablePath: \"${DEST}\" })"
echo "MCP: { \"mcpServers\": { \"playwright\": { \"command\": \"lurien-mcp\" } } }"

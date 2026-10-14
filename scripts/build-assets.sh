#!/usr/bin/env bash
# Build committed UI CSS; no Node process is needed by the running Rust app.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
assets="$root/apps/base/ui/assets"
command -v npm >/dev/null || { echo 'Install Node.js >=22 and npm to build UI assets.' >&2; exit 1; }
if [[ ! -x "$assets/node_modules/.bin/tailwindcss" ]]; then
    npm ci --prefix "$assets" --no-audit --no-fund
fi
if [[ ${1:-} == --check ]]; then
    npm run check --prefix "$assets"
else
    npm run build --prefix "$assets"
fi

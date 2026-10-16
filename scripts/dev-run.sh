#!/usr/bin/env bash
# Used on the first launch and every watched rebuild. Cargo's own diagnostics
# remain visible, plus explicit wall-clock timing including failed compilations.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$root"
python3 scripts/discover-modules.py
./scripts/build-assets.sh
started=$(date +%s)
result=0
cargo build -p bilibi-web --timings || result=$?
echo "Rust build took $(($(date +%s)-started))s (exit $result). Timing report: target/cargo-timings/cargo-timing.html"
[[ "$result" == 0 ]] || exit "$result"
# A rebuild can carry a module's new migration; apply it before serving, as
# start-app.sh does on launch, so the running app never queries a schema the
# database lacks. The seed stays launch-only.
"$root/target/debug/bilibi-web" migrate
exec "$root/target/debug/bilibi-web" run

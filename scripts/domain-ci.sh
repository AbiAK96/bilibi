#!/usr/bin/env bash
# The mounted-container gates for a Domain or Extension repository's CI. Run it
# from a Platform checkout with the container already mounted at
# apps/<domains|extensions>/<container> (docs/domain-ci/ci.yml does that) and
# DATABASE_URL naming an empty PostgreSQL 18 database. Nothing here is a unit
# test: it builds the composed app, migrates a fresh database, and loads `/`.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$root"
: "${DATABASE_URL:?DATABASE_URL must name an empty PostgreSQL 18 database}"
step() { printf '\n== %s\n' "$*"; }

step 'Discovery refuses any container shape the Platform does not accept'
python3 scripts/discover-modules.py
python3 scripts/discover-modules.py --check

step 'Committed CSS already covers the mounted container (no CSS change when mounting)'
./scripts/build-assets.sh --check

# --locked cannot add the mounted crates to Cargo.lock; plain metadata does, and
# --pin then proves the lock resolves and the mounted composition is pinnable.
step 'Resolve the mounted composition into Cargo.lock and pin it'
cargo metadata --format-version 1 >/dev/null
python3 scripts/discover-modules.py --pin
cat .local/composition/manifest.txt

step 'Format, lint and build every workspace member, mounted crates included'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked

step 'Migrate a fresh database and apply the production seeds'
app="${CARGO_TARGET_DIR:-$root/target}/release/bilibi-web"
"$app" migrate
"$app" seed-production

step 'Boot the composed app and load /'
port=${DOMAIN_CI_PORT:-3000}
BIND_ADDR="127.0.0.1:$port" APP_ORIGIN="http://localhost:$port" "$app" run &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do
    kill -0 "$server" 2>/dev/null || { echo 'The app exited during boot (module graph, contribution or migration refusal)' >&2; exit 1; }
    status=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/" || true)
    [[ $status =~ ^(200|30[1-8])$ ]] && { echo "GET / answered $status"; exit 0; }
    sleep 1
done
echo 'GET / never answered' >&2
exit 1

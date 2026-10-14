#!/usr/bin/env bash
# One development entrypoint; no daemon management or automatic installation.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$root"
for tool in rustc cargo psql pg_isready caddy mkcert python3 openssl curl ss flock sudo; do
    command -v "$tool" >/dev/null || { echo "Missing prerequisite: $tool" >&2; exit 1; }
done
cargo watch --version >/dev/null 2>&1 || { echo 'Install the rebuild watcher: cargo install cargo-watch --locked' >&2; exit 1; }
echo "$(rustc --version); $(cargo --version)"
python3 scripts/discover-modules.py
if [[ -z ${DATABASE_URL:-} ]]; then
    command -v docker >/dev/null || { echo 'Set DATABASE_URL or install Docker Compose for local PostgreSQL.' >&2; exit 1; }
    docker compose up -d --wait postgres
    export DATABASE_URL='postgres://bilibi:bilibi@localhost:5433/bilibi'
fi
pg_isready -d "$DATABASE_URL" >/dev/null || { echo 'PostgreSQL is not ready for DATABASE_URL.' >&2; exit 1; }
# A supplied URL must point at an existing DB; the default Compose path creates it.
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atc 'select 1' >/dev/null
if [[ -z ${SEED_PASSWORD:-} && ! -f "$root/.local/seed-password" ]]; then
    mkdir -p "$root/.local"
    (umask 077; openssl rand -hex 16 > "$root/.local/seed-password")
    echo 'Generated demo password in .local/seed-password (owner-readable only).'
fi
export SEED_PASSWORD=${SEED_PASSWORD:-$(cat "$root/.local/seed-password")}
started=$(date +%s)
cargo build -p bilibi-web --timings
echo "Initial Rust build took $(($(date +%s)-started))s. Timing report: target/cargo-timings/cargo-timing.html"
./target/debug/bilibi-web migrate
./target/debug/bilibi-web seed
unset SEED_PASSWORD
"$root/scripts/local-https.sh"
readarray -t instance < "$root/.local/https/instance"
export APP_ORIGIN="https://${instance[0]}"
export BIND_ADDR="127.0.0.1:${instance[1]}"
if [[ -n $(ss -H -ltn "sport = :${instance[1]}") ]]; then
    echo "This checkout already has a listener at $APP_ORIGIN. Stop its dev command before starting another." >&2
    exit 1
fi
echo "Open $APP_ORIGIN — tenant 1, manager@example.test (password in .local/seed-password or your SEED_PASSWORD)."
echo 'Cargo output stays visible. Edit Rust/templates/assets to rebuild; Ctrl-C stops this app, leaving shared Caddy running.'
exec cargo watch --watch apps --watch Cargo.toml --watch Cargo.lock --watch scripts --shell './scripts/dev-run.sh'

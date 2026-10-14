#!/usr/bin/env bash
# Privileged installation is separate from mkcert so it uses the developer's
# existing CA, not root's CA. Preserve and reload the shared Caddy service.
set -euo pipefail
[[ $# -eq 5 && ( $EUID -eq 0 || ${BILIBI_HTTPS_TEST:-0} == 1 ) ]] || { echo "Internal helper: call scripts/local-https.sh" >&2; exit 1; }
root=$1 host=$2 port=$3 clients=$4 pinned=$5
[[ "$root" == /* && -d "$root" && "$root" != *$'\n'* && "$root" != *'"'* ]] || exit 2
[[ "$port" =~ ^[0-9]{4,5}$ ]] && (( 10#$port >= 1024 && 10#$port <= 65535 )) || exit 2
port=$((10#$port))
[[ "$host" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+localhost$ ]] || exit 2
[[ "$port" =~ ^[0-9]{4,5}$ && "$clients" =~ ^[0-9a-fA-F.:[:space:]]+$ ]] || exit 2
caddy_root=${BILIBI_CADDY_ROOT:-/etc/caddy}
hosts_file=${BILIBI_HOSTS_FILE:-/etc/hosts}
lock_file=${BILIBI_CADDY_LOCK:-/run/lock/bilimbi-local-https.lock}
if [[ ${BILIBI_HTTPS_TEST:-0} == 1 ]]; then
    [[ "$caddy_root" == /tmp/* && "$hosts_file" == /tmp/* && "$lock_file" == /tmp/* ]] || exit 2
fi
exec 9>"$lock_file"
flock 9
main="$caddy_root/Caddyfile"
directory="$caddy_root/bilibi"
site="$directory/$host.caddy"
owner="# Bilibi checkout: $root"
if [[ -f "$site" && $(head -n 1 "$site") != "$owner" ]]; then
    previous=$(sed -n '1s/^# Bilibi checkout: //p' "$site")
    [[ -n "$previous" && ! -d "$previous" ]] || {
        echo "The Caddy site $host belongs to another checkout; choose a distinct BILIBI_DEV_HOST." >&2
        exit 1
    }
fi
ownership=(-o root -g caddy)
[[ ${BILIBI_HTTPS_TEST:-0} != 1 ]] || ownership=()
install -d "${ownership[@]}" -m 0750 "$directory/certs"
backup=$(mktemp -d "$caddy_root/.bilibi-backup.XXXXXX")
mkdir "$backup/stale-sites" "$backup/stale-certs"
cp -p "$main" "$backup/main"
[[ ! -f "$site" ]] || cp -p "$site" "$backup/site"
for suffix in .pem -key.pem; do
    [[ ! -f "$directory/certs/$host$suffix" ]] || cp -p "$directory/certs/$host$suffix" "$backup/cert$suffix"
done
rollback=1
cleanup() {
    if [[ "$rollback" == 1 ]]; then
        cp -p "$backup/main" "$main"
        if [[ -f "$backup/site" ]]; then cp -p "$backup/site" "$site"; else rm -f -- "$site"; fi
        for suffix in .pem -key.pem; do
            if [[ -f "$backup/cert$suffix" ]]; then
                cp -p "$backup/cert$suffix" "$directory/certs/$host$suffix"
            else
                rm -f -- "$directory/certs/$host$suffix"
            fi
        done
        for stale_site in "$backup"/stale-sites/*.caddy; do
            [[ -f "$stale_site" ]] || continue
            cp -p "$stale_site" "$directory/$(basename "$stale_site")"
        done
        for stale_cert in "$backup"/stale-certs/*; do
            [[ -f "$stale_cert" ]] || continue
            cp -p "$stale_cert" "$directory/certs/$(basename "$stale_cert")"
        done
        systemctl reload caddy >&2 || true
    fi
    find "$backup" -depth -type f -delete
    find "$backup" -depth -type d -empty -delete
    rmdir -- "$backup" 2>/dev/null || true
}
trap cleanup EXIT

# A checkout copy used for validation can disappear without running a
# shutdown hook. Retire only fragments whose recorded owner directory is gone.
while IFS= read -r -d '' fragment; do
    [[ -f "$fragment" ]] || continue
    stale_owner=$(sed -n '1s/^# Bilibi checkout: //p' "$fragment")
    [[ -n "$stale_owner" && ! -d "$stale_owner" ]] || continue
    stale_host=${fragment##*/}
    stale_host=${stale_host%.caddy}
    [[ "$stale_host" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+localhost$ ]] || continue
    cp -p "$fragment" "$backup/stale-sites/$stale_host.caddy"
    for suffix in .pem -key.pem; do
        stale_cert="$directory/certs/$stale_host$suffix"
        [[ ! -f "$stale_cert" ]] || cp -p "$stale_cert" "$backup/stale-certs/$stale_host$suffix"
    done
    rm -f -- "$fragment" "$directory/certs/$stale_host.pem" "$directory/certs/$stale_host-key.pem"
    printf '%s\n' "$stale_host" >> "$backup/stale-hosts"
done < <(find "$directory" -maxdepth 1 -type f -name '*.caddy' -print0)

available() {
    local candidate=$1 fragment listener pid
    # Installed fragments reserve ports even while another checkout compiles.
    while IFS= read -r -d '' fragment; do
        [[ -f "$fragment" && "$fragment" != "$site" ]] || continue
        if grep -Eq "reverse_proxy[[:space:]]+127\\.0\\.0\\.1:$candidate([[:space:]]|$)" "$fragment"; then return 1; fi
    done < <(find "$caddy_root" -maxdepth 2 -type f \( -name '*.caddy' -o -name Caddyfile \) -print0)
    listener=$(ss -H -ltnp "sport = :$candidate")
    [[ -n "$listener" ]] || return 0
    pid=$(printf '%s' "$listener" | sed -n 's/.*pid=\([0-9]*\).*/\1/p')
    # Reconfiguring the same site is safe; a new site cannot take its listener.
    [[ -f "$site" && -n "$pid" && $(readlink "/proc/$pid/cwd" 2>/dev/null || true) == "$root" ]]
}
while ! available "$port"; do
    if [[ "$pinned" == 1 ]]; then
        echo "PORT=$port is occupied or reserved by another instance; no process was stopped." >&2
        exit 1
    fi
    port=$((port + 1))
    (( port <= 65535 )) || { echo "No development port available" >&2; exit 1; }
done

install "${ownership[@]}" -m 0644 "$root/.local/https/$host.pem" "$directory/certs/$host.pem"
install "${ownership[@]}" -m 0640 "$root/.local/https/$host-key.pem" "$directory/certs/$host-key.pem"
cat > "$site" <<EOF
$owner
$host {
    tls "$directory/certs/$host.pem" "$directory/certs/$host-key.pem"
    @remote not remote_ip $clients
    respond @remote "Local development only" 403
    encode zstd gzip
    reverse_proxy 127.0.0.1:$port
}
EOF
chmod 0644 "$site"
if ! grep -Fqx "import $directory/*.caddy" "$main"; then
    printf '\n# Bilibi local development\nimport %s/*.caddy\n' "$directory" >> "$main"
fi
validator=(runuser -u caddy -- caddy)
[[ ${BILIBI_HTTPS_TEST:-0} != 1 ]] || validator=(caddy)
if ! "${validator[@]}" validate --config "$main" --adapter caddyfile > "$backup/validation" 2>&1; then
    cat "$backup/validation" >&2
    exit 1
fi
systemctl reload caddy
if [[ -f "$backup/stale-hosts" ]]; then
    while IFS= read -r stale_host; do
        python3 - "$stale_host" "$hosts_file" <<'PY'
from pathlib import Path
import os
import sys
import tempfile

hosts = Path(sys.argv[2])
line = f"127.0.0.1\t{sys.argv[1]} # Bilibi local HTTPS\n"
contents = hosts.read_text()
if line in contents:
    descriptor, temporary = tempfile.mkstemp(dir=hosts.parent, prefix=".bilimbi-hosts-")
    try:
        with os.fdopen(descriptor, "w") as output:
            output.write(contents.replace(line, ""))
        os.chmod(temporary, hosts.stat().st_mode & 0o777)
        os.replace(temporary, hosts)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
PY
    done < "$backup/stale-hosts"
fi
rollback=0
echo "Shared Caddy configuration validated and reloaded." >&2
printf '%s\n' "$port"

# Generic CLI resolvers use /etc/hosts; browsers also recognize .localhost.
if ! getent hosts "$host" >/dev/null; then
    printf '127.0.0.1\t%s # Bilibi local HTTPS\n' "$host" >> "$hosts_file"
fi

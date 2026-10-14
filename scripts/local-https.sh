#!/usr/bin/env bash
# mkcert runs as the developer; only its leaf key is installed for shared Caddy.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
host=${BILIBI_DEV_HOST:-bilibi.localhost}
port=${BILIBI_DEV_PORT:-4300}
fail() { echo "$*" >&2; exit 1; }
[[ "$host" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+localhost$ && ${#host} -le 253 ]] || fail 'Choose a lowercase BILIBI_DEV_HOST under .localhost.'
IFS=. read -r -a labels <<< "$host"
for label in "${labels[@]}"; do (( ${#label} <= 63 )) || fail 'Hostname label too long.'; done
if [[ ! "$port" =~ ^[0-9]{4,5}$ ]] || (( 10#$port < 1024 || 10#$port > 65535 )); then fail 'BILIBI_DEV_PORT must be 1024..65535.'; fi
for tool in caddy mkcert openssl curl python3 ss sudo flock sha256sum systemctl; do
    command -v "$tool" >/dev/null || fail "Missing $tool. On Ubuntu/WSL: sudo apt install caddy mkcert libnss3-tools postgresql-client"
done
systemctl is-active --quiet caddy || fail 'Start the shared service first: sudo systemctl start caddy. Enable systemd on WSL2.'
mkdir -p "$root/.local/https"
exec 8>"/tmp/bilibi-local-https-$(id -u).lock"
flock 8
site="/etc/caddy/bilibi/$host.caddy"
if [[ -f "$site" && $(head -n 1 "$site") != "# Bilibi checkout: $root" ]]; then
    recorded=$(sed -n '1s/^# Bilibi checkout: //p' "$site")
    if [[ -z "$recorded" || -d "$recorded" ]]; then
        [[ -z ${BILIBI_DEV_HOST:-} ]] || fail "The hostname $host belongs to another checkout."
        identifier=$(printf '%s' "$root" | sha256sum | cut -c1-10)
        host="bilibi-$identifier.localhost"
    fi
fi
mkcert -install
ca_root=$(mkcert -CAROOT)
if command -v powershell.exe >/dev/null && command -v wslpath >/dev/null; then
    powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
        -File "$(wslpath -w "$root/scripts/trust-local-ca.ps1")" \
        -CertificatePath "$(wslpath -w "$ca_root/rootCA.pem")"
fi
certificate="$root/.local/https/$host.pem"
key="$root/.local/https/$host-key.pem"
if [[ ! -f "$certificate" || ! -f "$key" ]] || \
    ! openssl x509 -in "$certificate" -checkend 2592000 -noout >/dev/null 2>&1 || \
    ! openssl verify -CAfile "$ca_root/rootCA.pem" -verify_hostname "$host" "$certificate" >/dev/null 2>&1; then
    (umask 077; mkcert -cert-file "$certificate" -key-file "$key" "$host")
fi
clients='127.0.0.1 ::1'
if command -v wslinfo >/dev/null && [[ $(wslinfo --networking-mode) == nat ]]; then
    gateway=$(ip -4 route show default | awk 'NR == 1 {print $3}')
    [[ "$gateway" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'Could not resolve Windows WSL gateway.'
    clients+=" $gateway"
fi
pinned=0
[[ -z ${BILIBI_DEV_PORT:-} ]] || pinned=1
port=$(sudo bash "$root/scripts/local-https/install-caddy.sh" "$root" "$host" "$port" "$clients" "$pinned")
printf '%s\n%s\n' "$host" "$port" > "$root/.local/https/instance"
echo "Local HTTPS configured: https://$host → 127.0.0.1:$port"

#!/bin/sh
# ATM cross-host transfer script -- Tailscale variant.
#
# Use this instead of sftp.sh when <host> is only reachable by its Tailscale
# MagicDNS name rather than a LAN/VPN hostname. Tailscale's contribution here
# is reachability (MagicDNS + WireGuard mesh, running an ordinary SSH server
# on the destination, or Tailscale SSH if you have that feature enabled) --
# the actual file transfer below is still plain `scp`/`ssh`, so this script
# is nearly identical to sftp.sh with a differently-resolved `<host>`.
#
# Install as ~/.atm/transfer/<host> with:
#   cp tailscale.sh ~/.atm/transfer/<host>
#   chmod 700 ~/.atm/transfer/<host>
#
# Same invocation contract as sftp.sh: argv-array `<script> <host>
# <transfer-id> <file>...`, restricted environment (ATM_TEMP, ATM_IDENTITY,
# ATM_TEAM only), closed stdin, bounded deadline. Success is exactly one
# line on stdout: the landed directory's absolute path.
set -eu

host="$1"
transfer_id="$2"
shift 2

# `<host>` here is the Tailscale MagicDNS name (for example `rand-m5`),
# which is also what a `HostName` roster entry should record for a
# Tailscale-only destination (`teams update-member --host rand-m5`).
#
# Remote $ATM_TEMP resolution -- pick ONE and delete the other:
remote_atm_temp="/tmp/atm-$(id -u)"
# remote_atm_temp="$(ssh "$host" 'echo "$ATM_TEMP"')"

remote_dir="$remote_atm_temp/send-to/$transfer_id"

# Ordinary SSH addressed at the tailnet hostname; the tailnet already
# authenticated this connection, so no separate key management is needed
# beyond the fleet's usual passwordless-SSH baseline.
ssh "$host" "umask 077 && mkdir -p '$remote_dir'"

for file in "$@"; do
    scp -q "$file" "$host:$remote_dir/"
done

printf '%s\n' "$remote_dir"

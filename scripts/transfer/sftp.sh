#!/bin/sh
# ATM cross-host transfer script -- sftp over passwordless SSH (default).
#
# Install as ~/.atm/transfer/<host> (extensionless, exactly the destination
# HostName you send to) with:
#   cp sftp.sh ~/.atm/transfer/<host>
#   chmod 700 ~/.atm/transfer/<host>
#
# ATM invokes this script directly (argv-array, never through a shell) as:
#   <script> <host> <transfer-id> <file>...
# with a restricted environment containing ATM_TEMP, ATM_IDENTITY, and
# ATM_TEAM (plus ATM_TRANSFER_SSH_CONFIG, an opt-in fourth entry below --
# unset for every ordinary install), your current working directory, and
# closed stdin. A bounded deadline applies (default 60s); a wedged script is
# killed.
#
# Contract: on success, print EXACTLY ONE LINE to stdout -- the absolute
# path of the directory the files now live in on <host> -- and exit 0. On
# any failure, print a short diagnostic to stderr and exit non-zero. Do not
# print anything else to stdout; ATM treats stdout as untrusted input and
# rejects multi-line, relative, or control-character output as a transfer
# failure.
set -eu

host="$1"
transfer_id="$2"
shift 2

# Optional: route ssh/scp through a scratch config (`ssh -F <path>`)
# instead of the real `~/.ssh/config`. Ordinary installs never set
# ATM_TRANSFER_SSH_CONFIG, so ssh_cmd/scp_cmd behave exactly like plain
# `ssh`/`scp` below; test/tooling harnesses (for example
# `scripts/phase-aq/run_aq4_transfer_evidence.py`) export it to point at a
# throwaway config for a loopback sshd without ever touching the real
# `~/.ssh/config`.
if [ -n "${ATM_TRANSFER_SSH_CONFIG:-}" ]; then
    ssh_cmd() { ssh -F "$ATM_TRANSFER_SSH_CONFIG" "$@"; }
    scp_cmd() { scp -F "$ATM_TRANSFER_SSH_CONFIG" "$@"; }
else
    ssh_cmd() { ssh "$@"; }
    scp_cmd() { scp "$@"; }
fi

# Remote $ATM_TEMP resolution -- pick ONE of the following two approaches
# and delete the other. Both assume this fleet's baseline: passwordless SSH
# from this machine to <host>.
#
# (a) Fixed value, if every host in your fleet uses the same scratch root
#     (the ATM_TEMP default is per-uid, so this only works when the same
#     uid owns the destination account too):
remote_atm_temp="/tmp/atm-$(id -u)"
#
# (b) Ask the remote host what it actually resolved (uncomment to use this
#     instead of the fixed value above):
# remote_atm_temp="$(ssh_cmd "$host" 'echo "$ATM_TEMP"')"

remote_dir="$remote_atm_temp/send-to/$transfer_id"

# The destination directory is created by this script, not by the daemon.
ssh_cmd "$host" "umask 077 && mkdir -p '$remote_dir'"

for file in "$@"; do
    scp_cmd -q "$file" "$host:$remote_dir/"
done

# Exactly one line: the landed directory's absolute path.
printf '%s\n' "$remote_dir"

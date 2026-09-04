#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# Nautilus Scripts run with no visible terminal, so a failure that only
# reaches stderr is otherwise silent to the human who invoked this. Surface
# it via notify-send when available, in addition to stderr (never instead of
# it) -- stderr is what Nautilus itself may still log/show, and is the only
# channel left when notify-send is not installed.
stderr_file=$(mktemp)
trap 'rm -f "$stderr_file"' EXIT

set +e
"$script_dir/atm-send-to.sh" "$@" 2>"$stderr_file"
status=$?
set -e

cat "$stderr_file" >&2
if [ "$status" -eq 0 ]; then
    exit 0
fi

message=$(tail -n 1 "$stderr_file" 2>/dev/null || true)
[ -n "$message" ] || message="ATM Send-To failed (exit $status)"
if command -v notify-send >/dev/null 2>&1; then
    notify-send "ATM Send-To" "$message" >/dev/null 2>&1 || true
fi
exit "$status"

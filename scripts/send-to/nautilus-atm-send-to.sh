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
# Test-only seam (same contract as atm-send-to.command): when set, replaces
# the `notify-send` desktop notification below with a non-UI action so
# .just/tests/test_send_to_surface.py never raises a real notification.
# `none` suppresses it, `stderr` prints the notification text to stderr, and
# any other value is run as `<command> "ATM Send-To" "$message"`. Production
# launches never set this; the notify-send branch below stays the default.
notifier=${ATM_SEND_TO_NOTIFIER:-}
if [ -n "$notifier" ]; then
    case "$notifier" in
        none) ;;
        stderr) printf 'ATM Send-To: %s\n' "$message" >&2 ;;
        *) "$notifier" "ATM Send-To" "$message" >/dev/null 2>&1 || true ;;
    esac
elif command -v notify-send >/dev/null 2>&1; then
    notify-send "ATM Send-To" "$message" >/dev/null 2>&1 || true
fi
exit "$status"

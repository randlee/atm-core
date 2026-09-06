#!/bin/sh
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# Finder Quick Actions have no visible terminal, so a failure that only
# reaches stderr is otherwise silent to the human who invoked this. Surface
# it as a native macOS notification in addition to stderr (never instead of
# it): stderr still carries the full detail for anyone who does have a
# terminal attached (e.g. a Shortcut's "Run Shell Script" log).
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
# Test-only seam: when set, replaces the native `osascript display
# notification` below with a non-UI action so the wrapper's failure path can
# be exercised by .just/tests/test_send_to_surface.py without popping a real
# notification on the developer's desktop. `none` suppresses the
# notification, `stderr` prints the notification text to stderr, and any
# other value is run as `<command> "ATM Send-To" "$message"`. Production
# launches never set this; the osascript branch below stays the default.
notifier=${ATM_SEND_TO_NOTIFIER:-}
if [ -n "$notifier" ]; then
    case "$notifier" in
        none) ;;
        stderr) printf 'ATM Send-To: %s\n' "$message" >&2 ;;
        *) "$notifier" "ATM Send-To" "$message" >/dev/null 2>&1 || true ;;
    esac
elif command -v osascript >/dev/null 2>&1; then
    escaped=$(printf '%s' "$message" | sed 's/\\/\\\\/g; s/"/\\"/g')
    osascript -e "display notification \"$escaped\" with title \"ATM Send-To\"" >/dev/null 2>&1 || true
fi
exit "$status"

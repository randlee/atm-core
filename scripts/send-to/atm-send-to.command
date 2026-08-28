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
if command -v osascript >/dev/null 2>&1; then
    escaped=$(printf '%s' "$message" | sed 's/\\/\\\\/g; s/"/\\"/g')
    osascript -e "display notification \"$escaped\" with title \"ATM Send-To\"" >/dev/null 2>&1 || true
fi
exit "$status"

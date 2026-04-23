#!/bin/bash
# atm-nudge.sh <recipient>
#
# Post-send hook for ATM: nudges a named agent's tmux pane when a message
# is delivered to them. The recipient is matched against tmux pane titles.
#
# Usage (from [[atm.post_send_hooks]] in .atm.toml):
#   command = ["scripts/atm-nudge.sh", "team-lead"]
#   command = ["scripts/atm-nudge.sh", "arch-ctm"]

set -euo pipefail

RECIPIENT="${1:-}"
if [[ -z "$RECIPIENT" ]]; then
    echo "usage: atm-nudge.sh <recipient>" >&2
    exit 1
fi

# Resolve ATM_TEAM from environment or .atm.toml default
TEAM="${ATM_TEAM:-atm-dev}"
MESSAGE="You have unread ATM messages. Run: atm read --team ${TEAM}"
LOG_FILE="/tmp/atm-nudge.log"
TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# Find pane by title (pane title is set to the agent name by rmux)
PANE_ID=$(tmux list-panes -a \
    -F '#{pane_title}\t#{pane_id}' 2>/dev/null \
    | awk -F'\t' -v name="$RECIPIENT" '$1 == name { print $2; exit }')

if [[ -z "$PANE_ID" ]]; then
    printf '%s recipient=%s not found in any tmux pane\n' "$TIMESTAMP" "$RECIPIENT" >> "$LOG_FILE"
    exit 0
fi

BUFFER="atm-nudge-$$"
tmux set-buffer -b "$BUFFER" -- "$MESSAGE"
tmux paste-buffer -b "$BUFFER" -t "$PANE_ID"
tmux send-keys -t "$PANE_ID" Enter
tmux delete-buffer -b "$BUFFER" >/dev/null 2>&1 || true

printf '%s nudged recipient=%s pane=%s\n' "$TIMESTAMP" "$RECIPIENT" "$PANE_ID" >> "$LOG_FILE"

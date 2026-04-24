#!/usr/bin/env bash
# atm-nudge.sh <recipient>
#
# Post-send hook for ATM: nudges a named agent's tmux pane.
# Lookup strategy (in order):
#   1. Static map: SESSION:WINDOW.INDEX from ATM_NUDGE_<RECIPIENT_UPPER> env var
#   2. ATM_IDENTITY env var match via tmux show-environment
#   3. Pane title match (legacy fallback)

set -euo pipefail

RECIPIENT="${1:-}"
if [[ -z "$RECIPIENT" ]]; then
    echo "usage: atm-nudge.sh <recipient>" >&2
    exit 1
fi

TEAM="${ATM_TEAM:-atm-dev}"
MESSAGE="You have unread ATM messages. Run: atm read --team ${TEAM}"
LOG_FILE="/tmp/atm-nudge.log"
TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# Strategy 1: explicit env override ATM_NUDGE_ARCH_CTM, ATM_NUDGE_TEAM_LEAD, etc.
ENV_KEY="ATM_NUDGE_$(echo "$RECIPIENT" | tr '[:lower:]-' '[:upper:]_')"
PANE_TARGET="${!ENV_KEY:-}"

if [[ -n "$PANE_TARGET" ]]; then
    PANE_ID=$(tmux list-panes -t "$PANE_TARGET" -F '#{pane_id}' 2>/dev/null | head -1)
fi

# Strategy 2: match by pane command in known session:window
# Reads ATM_NUDGE_SESSION (default: atm-dev) and ATM_NUDGE_WINDOW (default: agents)
if [[ -z "${PANE_ID:-}" ]]; then
    SESSION="${ATM_NUDGE_SESSION:-atm-dev}"
    WINDOW="${ATM_NUDGE_WINDOW:-agents}"
    case "$RECIPIENT" in
        arch-ctm)
            # Codex runs as node; find the node pane in the agents window
            PANE_ID=$(tmux list-panes -t "${SESSION}:${WINDOW}" \
                -F '#{pane_id} #{pane_current_command}' 2>/dev/null \
                | awk '$2 == "node" { print $1; exit }')
            ;;
        team-lead)
            # First Claude (non-node) pane in agents window
            PANE_ID=$(tmux list-panes -t "${SESSION}:${WINDOW}" \
                -F '#{pane_id} #{pane_current_command}' 2>/dev/null \
                | awk '$2 != "node" { print $1; exit }')
            ;;
        quality-mgr)
            # Last Claude (non-node) pane in agents window
            PANE_ID=$(tmux list-panes -t "${SESSION}:${WINDOW}" \
                -F '#{pane_id} #{pane_current_command}' 2>/dev/null \
                | awk '$2 != "node" { last=$1 } END { print last }')
            ;;
    esac
fi

# Strategy 3: pane title fallback
if [[ -z "${PANE_ID:-}" ]]; then
    PANE_ID=$(tmux list-panes -a \
        -F '#{pane_title}\t#{pane_id}' 2>/dev/null \
        | awk -F'\t' -v name="$RECIPIENT" '$1 == name { print $2; exit }')
fi

if [[ -z "${PANE_ID:-}" ]]; then
    printf '%s recipient=%s not found in any tmux pane\n' "$TIMESTAMP" "$RECIPIENT" >> "$LOG_FILE"
    exit 0
fi

BUFFER="atm-nudge-$$"
tmux set-buffer -b "$BUFFER" -- "$MESSAGE"
tmux paste-buffer -b "$BUFFER" -t "$PANE_ID"
tmux send-keys -t "$PANE_ID" Enter
tmux delete-buffer -b "$BUFFER" >/dev/null 2>&1 || true

printf '%s nudged recipient=%s pane=%s\n' "$TIMESTAMP" "$RECIPIENT" "$PANE_ID" >> "$LOG_FILE"

#!/usr/bin/env bash
set -euo pipefail

recipient="${1:?recipient required}"
message="You have unread ATM messages. Run: atm read --team atm-dev"

case "$recipient" in
    team-lead)
        pane="atm-dev:1.1"
        ;;
    arch-ctm)
        pane="atm-dev:1.2"
        ;;
    *)
        exit 0
        ;;
esac

tmux send-keys -t "$pane" -l "$message"
sleep 0.5
tmux send-keys -t "$pane" Enter

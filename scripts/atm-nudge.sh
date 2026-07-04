#!/usr/bin/env bash
# atm-nudge.sh [recipient]
#
# Payload-driven post-send hook helper for ATM.
# The authoritative pane id must come from the ATM_POST_SEND payload.

set -euo pipefail

RECIPIENT_ARG="${1:-}"
PAYLOAD="${ATM_POST_SEND:-}"
if [[ -z "$PAYLOAD" ]]; then
    PAYLOAD="$(cat)"
fi
if [[ -z "$PAYLOAD" ]]; then
    echo "ATM_POST_SEND payload is required" >&2
    exit 1
fi

readarray -t PAYLOAD_FIELDS < <(
    python3 - <<'PY' "$PAYLOAD" "$RECIPIENT_ARG"
import json
import sys

payload = json.loads(sys.argv[1])
recipient = payload.get("recipient") or sys.argv[2]
team = payload.get("team") or ""
pane = payload.get("recipient_pane_id") or ""
print(recipient)
print(team)
print(pane)
PY
)

RECIPIENT="${PAYLOAD_FIELDS[0]:-}"
TEAM="${PAYLOAD_FIELDS[1]:-${ATM_TEAM:-atm-dev}}"
PANE_ID="${PAYLOAD_FIELDS[2]:-}"
MESSAGE="You have unread ATM messages. Run: atm read --team ${TEAM}"
LOG_FILE="${TMPDIR:-/tmp}/atm-nudge.log"
TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

if [[ -z "${PANE_ID:-}" ]]; then
    printf '%s recipient=%s missing authoritative pane id in ATM_POST_SEND payload\n' "$TIMESTAMP" "$RECIPIENT" >> "$LOG_FILE"
    exit 1
fi

tmux send-keys -t "$PANE_ID" -l "$MESSAGE"
tmux send-keys -t "$PANE_ID" Enter

printf '%s nudged recipient=%s pane=%s\n' "$TIMESTAMP" "$RECIPIENT" "$PANE_ID" >> "$LOG_FILE"

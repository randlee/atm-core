#!/usr/bin/env bash
# atm-nudge.sh [recipient]
#
# Compatibility / explicit-override post-send helper for ATM.
# This is not the shipped default nudge path; the built-in default is
# `atm internal-nudge`. The authoritative pane id must come from the
# ATM_POST_SEND payload.

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
from_value = payload.get("from") or ""
message_id = payload.get("message_id") or ""
task_id = payload.get("task_id") or ""
description = payload.get("description") or payload.get("summary") or ""
requires_ack = payload.get("requires_ack") is True
is_ack = payload.get("is_ack") is True

attrs = []
if from_value:
    attrs.append(f'from="{from_value}"')
if message_id:
    attrs.append(f'message-id="{message_id}"')
base = "<atm" + (f" {' '.join(attrs)}" if attrs else "")

if is_ack:
    if task_id:
        message = f'{base} kind="ack" task-id="{task_id}"/>'
    else:
        message = f'{base} kind="ack"/>'
else:
    read_action = f"atm read --message-id {message_id}" if message_id else "atm read"
    parts = [f"{base}>", f"<action>{read_action}</action>"]
    if requires_ack:
        parts.append("<action>ack the message</action>")
    if task_id:
        parts.append(f'<task id="{task_id}">{description}</task>')
    else:
        parts.append(f"<description>{description}</description>")
    parts.extend(
        [
            "<action>execute the assigned task</action>",
            '<when idle="immediate" busy="after-current-task"/>',
            '<console announce="concise" pause="false"/>',
            "</atm>",
        ]
    )
    message = "".join(parts)

print(recipient)
print(team)
print(pane)
print(message)
PY
)

RECIPIENT="${PAYLOAD_FIELDS[0]:-}"
TEAM="${PAYLOAD_FIELDS[1]:-${ATM_TEAM:-atm-dev}}"
PANE_ID="${PAYLOAD_FIELDS[2]:-}"
MESSAGE="${PAYLOAD_FIELDS[3]:-}"
LOG_FILE="${TMPDIR:-/tmp}/atm-nudge.log"
TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

if [[ -z "${PANE_ID:-}" ]]; then
    printf '%s recipient=%s missing authoritative pane id in ATM_POST_SEND payload\n' "$TIMESTAMP" "$RECIPIENT" >> "$LOG_FILE"
    exit 1
fi
if [[ -z "${MESSAGE:-}" ]]; then
    printf '%s recipient=%s missing rendered nudge message\n' "$TIMESTAMP" "$RECIPIENT" >> "$LOG_FILE"
    exit 1
fi

tmux send-keys -t "$PANE_ID" -l "$MESSAGE"
tmux send-keys -t "$PANE_ID" Enter
sleep 0.25
tmux send-keys -t "$PANE_ID" Enter

printf '%s nudged recipient=%s pane=%s\n' "$TIMESTAMP" "$RECIPIENT" "$PANE_ID" >> "$LOG_FILE"

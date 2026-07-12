#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
import os

payload = json.loads(os.environ["ATM_POST_SEND"])
print(json.dumps({
    "recipient": payload["recipient"],
    "message_id": payload["message_id"],
    "requires_ack": payload["requires_ack"],
    "is_ack": payload["is_ack"],
}))
PY

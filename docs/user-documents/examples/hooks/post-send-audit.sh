#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
import os

payload = json.loads(os.environ["ATM_POST_SEND"])
print(json.dumps({
    "sender": payload["sender"],
    "recipient": payload["recipient"],
    "message_id": payload["message_id"],
    "task_id": payload["task_id"],
}))
PY

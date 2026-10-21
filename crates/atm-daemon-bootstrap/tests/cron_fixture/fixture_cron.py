import json
from pathlib import Path
import sys

TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 30000


if __name__ == "__main__":
    request = json.load(sys.stdin)
    hook = sys.argv[1]
    with Path("cron_requests.jsonl").open("a", encoding="utf-8") as output:
        output.write(json.dumps({"request": request, "argv": sys.argv[1:]}) + "\n")
    if hook == "cron.stop":
        reply = {"stop": True, "reason": "marker present"} if Path("stop.marker").exists() else {"stop": False}
    else:
        reply = {"result": "pass-through"}
    print(json.dumps(reply))

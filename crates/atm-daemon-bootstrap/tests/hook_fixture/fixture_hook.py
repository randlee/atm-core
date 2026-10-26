import json
import os
from pathlib import Path
import sys

TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 30000


def fixture_reply():
    return json.loads(Path("fixture_reply.json").read_text(encoding="utf-8"))


def cron(args):
    return fixture_reply()


def stop(args):
    return fixture_reply()


def doctor(request):
    return fixture_reply()


if __name__ == "__main__":
    raw = sys.stdin.buffer.read()
    Path("fixture_request.json").write_bytes(raw)
    Path("fixture_env.json").write_text(json.dumps(dict(os.environ)), encoding="utf-8")
    Path("fixture_argv.json").write_text(json.dumps(sys.argv[1:]), encoding="utf-8")
    Path("fixture_cwd.txt").write_text(os.getcwd(), encoding="utf-8")
    request = json.loads(raw)
    hook = sys.argv[1]
    hooks_path = Path("fixture_hooks.txt")
    hooks = hooks_path.read_text(encoding="utf-8").splitlines() if hooks_path.exists() else ["task.close"]
    if hook == "doctor":
        result = doctor(request)
    elif hook == "cron.tick":
        result = cron(request["request"]["args"])
    elif hook == "cron.stop":
        result = stop(request["request"]["args"])
    elif hook in hooks:
        result = fixture_reply()
    else:
        result = {"result": "not-applicable"}
    print(json.dumps(result))

"""Deterministic cron replies for composed CLI integration tests.

The scheduled arguments select exactly one valid reply. Task and acknowledgement
identifiers are explicit so fixture execution has no hidden state.
"""

import json
import sys


TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 2500

TEAM = "test-team"
SENDER = "sender-a"
HOME = "/fixture/home"
CWD = "/fixture/repo"


def write_request(
    message,
    task_id=None,
    task_op=None,
    acknowledges_message_id=None,
    recipient="recipient",
):
    request = {
        "home_dir": HOME,
        "current_dir": CWD,
        "caller_identity": SENDER,
        "caller_team": TEAM,
        "to": {"agent": recipient, "team": TEAM}
        if acknowledges_message_id is None
        else None,
        "message_source": {"Inline": message},
        "requires_ack": False,
        "task_id": task_id,
        "dry_run": False,
        "nudge_mode": "immediate",
    }
    if task_op is not None:
        request["task_op"] = task_op
    if acknowledges_message_id is not None:
        request["acknowledges_message_id"] = acknowledges_message_id
    return request


def action_reply(command, request, run_as=None):
    action = {"command": command, "request": request}
    if run_as is not None:
        action["as"] = run_as
    return {
        "result": "handled",
        "atm-action": [action],
        "cron-output": [{"kind": "message", "body": f"fixture action {command}"}],
    }


def reply(argv):
    if argv == ["custom-mail"]:
        return {
            "result": "pass-through",
            "cron-output": [{"kind": "message", "body": "fixture custom mail"}],
        }
    if len(argv) != 2:
        raise SystemExit("expected one mode or one mode plus an identifier")
    mode, identifier = argv
    if mode == "task-assign":
        return action_reply(
            "task.assign", write_request("fixture task assignment", task_id=identifier)
        )
    if mode == "task-start":
        return action_reply(
            "task.start",
            write_request(
                "fixture task start", task_id=identifier, task_op={"op": "start"}
            ),
            run_as="recipient",
        )
    if mode == "task-close":
        return action_reply(
            "task.close",
            write_request(
                "fixture task completion",
                task_id=identifier,
                task_op={"op": "close", "outcome": "completed", "reason": "fixture"},
                recipient=SENDER,
            ),
            run_as="recipient",
        )
    if mode == "ack":
        return action_reply(
            "ack",
            write_request(
                "fixture acknowledgement", acknowledges_message_id=identifier
            ),
            run_as="recipient",
        )
    raise SystemExit(f"unknown fixture mode: {mode}")


if sys.argv[1:] != ["cron.tick"]:
    raise SystemExit("expected cron.tick hook verb")

request = json.load(sys.stdin)
args = request.get("request", {}).get("args")
if not isinstance(args, list) or not all(isinstance(arg, str) for arg in args):
    raise SystemExit("expected cron request args")

json.dump(reply(args), sys.stdout, separators=(",", ":"))
sys.stdout.write("\n")

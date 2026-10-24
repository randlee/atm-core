"""Deterministic cron replies for composed CLI integration tests.

The first argument selects exactly one valid reply. Task and acknowledgement
identifiers are explicit argv values so fixture execution has no hidden state.
"""

import json
import sys


TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 2500

TEAM = "test-team"
SENDER = "sender-a"
RECIPIENT = "recipient@test-team"
HOME = "/fixture/home"
CWD = "/fixture/repo"


def write_request(message, task_id=None, task_op=None, acknowledges_message_id=None):
    request = {
        "home_dir": HOME,
        "current_dir": CWD,
        "caller_identity": SENDER,
        "caller_team": TEAM,
        "to": RECIPIENT if acknowledges_message_id is None else None,
        "message_source": {"Inline": message},
        "requires_ack": False,
        "task_id": task_id,
        "dry_run": False,
    }
    if task_op is not None:
        request["task_op"] = task_op
    if acknowledges_message_id is not None:
        request["acknowledges_message_id"] = acknowledges_message_id
    return request


def action_reply(command, request):
    return {
        "result": "handled",
        "atm-action": [{"command": command, "request": request}],
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
        )
    if mode == "task-close":
        return action_reply(
            "task.close",
            write_request(
                "fixture task completion",
                task_id=identifier,
                task_op={"op": "close", "outcome": "completed", "reason": "fixture"},
            ),
        )
    if mode == "ack":
        return action_reply(
            "ack",
            write_request(
                "fixture acknowledgement", acknowledges_message_id=identifier
            ),
        )
    raise SystemExit(f"unknown fixture mode: {mode}")


json.dump(reply(sys.argv[1:]), sys.stdout, separators=(",", ":"))
sys.stdout.write("\n")

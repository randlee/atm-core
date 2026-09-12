#!/usr/bin/env python3
"""Run the BB.4 task-start CLI smoke cases against a live testbed container.

The runner talks only to the public ``atm`` CLI inside an already-running
container.  It writes its complete command transcripts and conclusions under
``site/reports`` so the evidence cannot be mistaken for a hand-edited report.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TEAM = "testbed"
ASSIGNER = "stub-alpha"
ASSIGNEE = "tester"
POLL_SECONDS = 5.0
REMINDER_TIMEOUT_SECONDS = 135.0


def run_cli(container: str, identity: str, args: list[str], timeout: float = 30.0) -> dict[str, Any]:
    command = [
        "docker",
        "exec",
        "-e",
        f"ATM_IDENTITY={identity}",
        "-e",
        f"ATM_TEAM={TEAM}",
        container,
        "atm",
        *args,
    ]
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    return {
        "command": " ".join(command),
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def json_value(result: dict[str, Any], label: str) -> Any:
    if result["exit_code"] != 0:
        raise RuntimeError(f"{label} failed: {result['stderr'].strip() or result['stdout'].strip()}")
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} did not return JSON: {error}") from error


def task_rows(value: Any) -> list[dict[str, Any]]:
    rows = value.get("rows") if isinstance(value, dict) else value
    return [row for row in rows if isinstance(row, dict)] if isinstance(rows, list) else []


def message_rows(value: Any) -> list[dict[str, Any]]:
    rows = value.get("rows") if isinstance(value, dict) else value
    return [row for row in rows if isinstance(row, dict)] if isinstance(rows, list) else []


def task_id(prefix: str, run_id: str, suffix: str) -> str:
    return f"{prefix}-{run_id[-12:]}-{suffix}"


def assign(container: str, task: str, description: str) -> dict[str, Any]:
    return run_cli(
        container,
        ASSIGNER,
        ["task", "assign", ASSIGNEE, "--task-id", task, "--team", TEAM, description],
    )


def close(container: str, task: str) -> dict[str, Any]:
    return run_cli(container, ASSIGNER, ["task", "close", task, "cancelled", "BB4 smoke cleanup"])


def task_start_line_reaches_assigner_at_write_time(container: str, run_id: str) -> dict[str, Any]:
    """Keep the assigner unread while a start is written, then inspect its mailbox."""
    task = task_id("BB4A", run_id, "start")
    assignment = assign(container, task, "BB4 immediate start line")
    start = run_cli(container, ASSIGNEE, ["task", "start", task, "--team", TEAM, "BB4 started at write time"])
    mailbox = run_cli(container, ASSIGNER, ["list", "--json"])
    messages = message_rows(json_value(mailbox, "assigner mailbox list")) if mailbox["exit_code"] == 0 else []
    selected = next((item for item in messages if item.get("task_id") == task), None)
    read: dict[str, Any] | None = None
    if selected is not None:
        read = run_cli(container, ASSIGNER, ["read", "--message-id", selected["message_id"], "--team", TEAM, "--json"])
    read_payload = json_value(read, "started message read") if read and read["exit_code"] == 0 else {}
    message = read_payload.get("message", {}) if isinstance(read_payload, dict) else {}
    task_op = message.get("taskOp") if isinstance(message, dict) else None
    passed = (
        assignment["exit_code"] == 0
        and start["exit_code"] == 0
        and mailbox["exit_code"] == 0
        and isinstance(selected, dict)
        and selected.get("from") == ASSIGNEE
        and selected.get("task_id") == task
        and read is not None
        and read["exit_code"] == 0
        and message.get("taskId") == task
        and message.get("from") == ASSIGNEE
        and isinstance(task_op, dict)
        and task_op.get("op") == "start"
    )
    return {
        "name": "task_start_line_reaches_assigner_at_write_time",
        "status": "PASS" if passed else "FAIL",
        "task_id": task,
        "assigner_was_not_read_until_after_start": True,
        "assignment": assignment,
        "start": start,
        "mailbox": mailbox,
        "selected_message": selected,
        "read": read,
        "observed_task_op": task_op,
    }


def task_start_out_of_order_reorders_queue(container: str, run_id: str) -> dict[str, Any]:
    """Start task 3 and prove the public list is head-first and active."""
    tasks = [task_id("BB4B", run_id, suffix) for suffix in ("one", "two", "three")]
    assignments = [assign(container, task, f"BB4 queue task {index}") for index, task in enumerate(tasks, 1)]
    start = run_cli(container, ASSIGNEE, ["task", "start", tasks[2], "--team", TEAM, "BB4 task three started"])
    listing = run_cli(container, ASSIGNER, ["task", "list", "--all", "--json"])
    rows = task_rows(json_value(listing, "all-task list")) if listing["exit_code"] == 0 else []
    relevant = [row for row in rows if row.get("task_id") in tasks]
    passed = (
        all(result["exit_code"] == 0 for result in assignments)
        and start["exit_code"] == 0
        and listing["exit_code"] == 0
        and [row.get("task_id") for row in relevant] == [tasks[2], tasks[0], tasks[1]]
        and relevant[0].get("position") == 1
        and relevant[0].get("state") == "active"
    )
    return {
        "name": "task_start_out_of_order_reorders_queue",
        "status": "PASS" if passed else "FAIL",
        "task_ids_in_assignment_order": tasks,
        "assignments": assignments,
        "start": start,
        "listing": listing,
        "relevant_rows": relevant,
    }


def prompted_task_never_started_stays_assigned_and_keeps_reminding(container: str, run_id: str) -> dict[str, Any]:
    """Do not start the task; wait for two reminder passes and inspect events."""
    task = task_id("BB4C", run_id, "reminder")
    assignment = assign(container, task, "BB4 prompted but never started")
    snapshots: list[dict[str, Any]] = []
    deadline = time.monotonic() + REMINDER_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        listing = run_cli(container, ASSIGNER, ["task", "list", "--all", "--json"])
        if listing["exit_code"] == 0:
            rows = task_rows(json_value(listing, "reminder task list"))
            selected = next((row for row in rows if row.get("task_id") == task), None)
            if selected is not None:
                snapshots.append({"observed_at": datetime.now(timezone.utc).isoformat(), "row": selected})
                if selected.get("reminder_count", 0) >= 2:
                    break
        time.sleep(POLL_SECONDS)
    events = run_cli(container, ASSIGNER, ["task", "events", task, "--team", TEAM, "--json"])
    event_rows = json_value(events, "task events") if events["exit_code"] == 0 else []
    started_events = [event for event in event_rows if isinstance(event, dict) and event.get("event") == "started"]
    last = snapshots[-1]["row"] if snapshots else {}
    reminder_counts = sorted({row["row"].get("reminder_count") for row in snapshots if isinstance(row.get("row"), dict)})
    passed = (
        assignment["exit_code"] == 0
        and events["exit_code"] == 0
        and last.get("state") == "assigned"
        and last.get("reminder_count", 0) >= 2
        and not started_events
    )
    return {
        "name": "prompted_task_never_started_stays_assigned_and_keeps_reminding",
        "status": "PASS" if passed else "FAIL",
        "task_id": task,
        "assignment": assignment,
        "poll_interval_seconds": POLL_SECONDS,
        "snapshots": snapshots,
        "reminder_counts_observed": reminder_counts,
        "events": events,
        "event_rows": event_rows,
        "started_events": started_events,
    }


def inspect_container(container: str) -> dict[str, Any]:
    inspection = subprocess.run(["docker", "inspect", container], capture_output=True, text=True, check=False)
    digest = None
    image = None
    if inspection.returncode == 0:
        try:
            record = json.loads(inspection.stdout)[0]
            image = record.get("Config", {}).get("Image")
            image_id = record.get("Image")
            if isinstance(image_id, str):
                image = image_id
        except (IndexError, KeyError, TypeError, json.JSONDecodeError):
            pass
    version = run_cli(container, ASSIGNER, ["--version"])
    members = run_cli(container, ASSIGNER, ["members", "--json", "--team", TEAM])
    payload = json_value(members, "member roster") if members["exit_code"] == 0 else {}
    names = [member.get("name") for member in payload.get("members", []) if isinstance(member, dict) and member.get("name")]
    return {"image": image, "version": version, "members": members, "roster": names}


def render_html(report: dict[str, Any]) -> str:
    rows = []
    for case in report["cases"]:
        detail = f"task {case.get('task_id', 'multiple')}"
        rows.append(
            f"<tr class=\"{escape(case['status'].lower())}\"><td>{'✓' if case['status'] == 'PASS' else '✗'}</td>"
            f"<td>{escape(case['name'])}</td><td>{escape(detail)}</td><td>{escape(case['status'])}</td></tr>"
        )
    return """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>BB.4 task-start CLI smoke</title>
<style>body{font:16px system-ui,sans-serif;max-width:72rem;margin:2rem auto;padding:0 1rem}table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #ddd;padding:.5rem;text-align:left}.pass{color:#176b2c}.fail{color:#a00}pre{white-space:pre-wrap;max-height:24rem;overflow:auto;background:#f5f5f5;padding:1rem}</style></head>
<body><h1>BB.4 task-start CLI smoke</h1>
<p>Runner-generated evidence. Source revision: <code>""" + escape(report["source_revision"]) + """</code></p>
<p>Container: <code>""" + escape(report["container"]) + """</code>; roster: <code>""" + escape(", ".join(report["roster"])) + """</code></p>
<table><thead><tr><th>Status</th><th>Case</th><th>Scope</th><th>Result</th></tr></thead><tbody>""" + "".join(rows) + """</tbody></table>
<h2>Machine-readable transcript</h2><pre>""" + escape(json.dumps(report, indent=2)) + """</pre></body></html>
"""


def run(container: str, out_dir: Path) -> int:
    generated_at = datetime.now(timezone.utc)
    run_id = generated_at.strftime("%Y%m%dT%H%M%S%fZ")
    source = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False)
    source_revision = source.stdout.strip() if source.returncode == 0 else "unknown"
    report: dict[str, Any] = {
        "feature": "bb4-task-start",
        "generated_at": generated_at.isoformat().replace("+00:00", "Z"),
        "source_revision": source_revision,
        "container": container,
        "cases": [],
    }
    inspection = inspect_container(container)
    report.update({"container_image": inspection["image"], "atm_version": inspection["version"], "roster": inspection["roster"]})
    cleanup_ids: list[str] = []
    cleanup_results: list[dict[str, Any]] = []

    def clean_tasks(tasks: list[str]) -> None:
        for task in dict.fromkeys(tasks):
            cleanup_ids.append(task)
            cleanup_results.append({"task_id": task, "result": close(container, task)})

    try:
        case_a = task_start_line_reaches_assigner_at_write_time(container, run_id)
        clean_tasks([case_a["task_id"]])
        report["cases"].append(case_a)
        case_b = task_start_out_of_order_reorders_queue(container, run_id)
        clean_tasks(case_b["task_ids_in_assignment_order"])
        report["cases"].append(case_b)
        case_c = prompted_task_never_started_stays_assigned_and_keeps_reminding(container, run_id)
        clean_tasks([case_c["task_id"]])
        report["cases"].append(case_c)
    finally:
        report["cleanup"] = cleanup_results
    report["status"] = "PASS" if report["cases"] and all(case["status"] == "PASS" for case in report["cases"]) else "FAIL"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "bb4-task-start.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    (out_dir / "index.html").write_text(render_html(report), encoding="utf-8")
    print(f"BB4 task-start smoke: {report['status']}")
    for case in report["cases"]:
        print(f"{case['status']} {case['name']}")
    print(f"evidence: {out_dir}")
    return 0 if report["status"] == "PASS" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", default="hermes-testbed", help="already-running Docker testbed container")
    parser.add_argument("--out", type=Path, help="evidence directory; defaults under site/reports")
    args = parser.parse_args()
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    out_dir = args.out or ROOT / "site/reports" / "bb4-task-start" / run_id
    try:
        return run(args.container, out_dir.resolve())
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        print(f"BB4 task-start smoke: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

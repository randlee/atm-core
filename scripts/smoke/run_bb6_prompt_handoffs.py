#!/usr/bin/env python3
"""Run the BB.6 prompt-handoff scenarios against a live colima fixture."""

from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import re
import subprocess
import sys
import time
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[2]
TEAM = "testbed"
DEFAULT_CONTAINER = "hermes-testbed-bb6"
ASSIGNER = "stub-alpha"
ASSIGNEE = "tester"
POLL_SECONDS = 0.5
WAIT_SECONDS = 90.0
REMINDER_WAIT_SECONDS = 65.0
SCENARIO_NAMES = (
    "task_events_shows_ready_reminders_and_started_for_prompted_task_only",
    "disabled_task_reminder_override_yields_no_handoff_and_doctor_finding",
)


def run_command(command: list[str], timeout: float = 45.0) -> dict[str, Any]:
    result = subprocess.run(
        command, capture_output=True, text=True, timeout=timeout, check=False
    )
    return {
        "command": " ".join(command),
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def run_cli(
    container: str, identity: str, args: list[str], timeout: float = 45.0
) -> dict[str, Any]:
    return run_command(
        [
            "docker",
            "exec",
            "-e",
            f"ATM_IDENTITY={identity}",
            "-e",
            f"ATM_TEAM={TEAM}",
            container,
            "atm",
            *args,
        ],
        timeout,
    )


def run_herdr(container: str, args: list[str]) -> dict[str, Any]:
    return run_command(["docker", "exec", container, "herdr", *args])


def json_value(result: dict[str, Any], label: str) -> Any:
    if result["exit_code"] != 0:
        detail = result["stderr"].strip() or result["stdout"].strip()
        raise RuntimeError(f"{label} failed: {detail}")
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} did not return JSON: {error}") from error


def task_id(run_id: str, suffix: str) -> str:
    return f"BB6-{run_id[-14:]}-{suffix}"


def assign(container: str, task: str, message: str) -> dict[str, Any]:
    return run_cli(
        container,
        ASSIGNER,
        ["task", "assign", ASSIGNEE, "--task-id", task, "--team", TEAM, message],
    )


def agent_pane(container: str) -> str:
    result = run_herdr(container, ["agent", "list"])
    payload = json_value(result, "herdr agent list")
    agents = payload.get("result", {}).get("agents", [])
    pane = next(
        (agent.get("pane_id") for agent in agents if agent.get("name") == ASSIGNEE),
        None,
    )
    if not isinstance(pane, str):
        raise RuntimeError(f"no pane for {ASSIGNEE} in herdr agent list")
    return pane


def pane_text(container: str, lines: int = 800) -> dict[str, Any]:
    return run_herdr(
        container,
        ["pane", "read", agent_pane(container), "--source", "recent-unwrapped", "--lines", str(lines)],
    )


def blocks_for(text: str, task: str) -> list[str]:
    pattern = re.compile(
        rf'<atm\b[^>]*\btask="{re.escape(task)}"[\s\S]*?(?:</atm>|/>)'
    )
    return pattern.findall(text)


def wait_for_pane(
    container: str, predicate: Callable[[str], bool], label: str
) -> dict[str, Any]:
    deadline = time.monotonic() + WAIT_SECONDS
    last = pane_text(container)
    while time.monotonic() < deadline:
        if last["exit_code"] == 0 and predicate(last["stdout"]):
            return last
        time.sleep(POLL_SECONDS)
        last = pane_text(container)
    raise RuntimeError(
        f"timed out waiting for {label}; last pane output: {last['stdout'][-2000:]}"
    )


def task_events(container: str, task: str) -> tuple[dict[str, Any], dict[str, Any]]:
    result = run_cli(
        container, ASSIGNER, ["task", "events", task, "--team", TEAM, "--json"]
    )
    payload = json_value(result, f"task events {task}")
    if not isinstance(payload, dict):
        raise RuntimeError(f"task events {task} returned a non-object payload")
    return result, payload


def handoff_kinds(payload: dict[str, Any]) -> list[str]:
    handoffs = payload.get("handoffs", [])
    if not isinstance(handoffs, list):
        return []
    return [row.get("kind") for row in handoffs if isinstance(row, dict)]


def event_names(payload: dict[str, Any]) -> list[str]:
    events = payload.get("events", [])
    if not isinstance(events, list):
        return []
    return [row.get("event") for row in events if isinstance(row, dict)]


def sqlite_count(container: str) -> dict[str, Any]:
    code = (
        "import sqlite3; "
        "db=sqlite3.connect('/root/.atm/db/mail.db'); "
        "print(db.execute('SELECT COUNT(*) FROM prompt_handoffs').fetchone()[0])"
    )
    return run_command(["docker", "exec", container, "python3", "-c", code])


def prompt_failure_count(container: str) -> dict[str, Any]:
    code = (
        "from pathlib import Path; "
        "p=Path('/root/.atm/logs/atm.log.jsonl'); "
        "print(sum('prompt_handoff_record_failed' in line for line in p.open()))"
    )
    return run_command(["docker", "exec", container, "python3", "-c", code])


def prompt_handoff_rows(container: str) -> dict[str, Any]:
    code = (
        "import json,sqlite3; "
        "db=sqlite3.connect('/root/.atm/db/mail.db'); "
        "rows=db.execute('SELECT agent,kind,task_id,attempt,message_key "
        "FROM prompt_handoffs ORDER BY rowid').fetchall(); "
        "print(json.dumps(rows))"
    )
    return run_command(["docker", "exec", container, "python3", "-c", code])


def integer_result(result: dict[str, Any], label: str) -> int:
    if result["exit_code"] != 0:
        raise RuntimeError(f"{label} failed: {result['stderr'].strip()}")
    try:
        return int(result["stdout"].strip())
    except ValueError as error:
        raise RuntimeError(f"{label} returned a non-integer") from error


def pane_terminal_lines(text: str, tasks: list[str]) -> list[dict[str, Any]]:
    lines = []
    for task in tasks:
        for block in blocks_for(text, task):
            message = re.search(r'\bmessage="([^"]+)"', block)
            if message is None:
                raise RuntimeError(f"task terminal has no message id: {task}")
            if match := re.search(r'\breminder="(\d+)"', block):
                kind, attempt = "task_reminder", int(match.group(1))
            elif re.search(r"\bready(?:\s|>)", block):
                kind, attempt = "task_ready", 0
            elif re.search(r'\bqueued="\d+"', block):
                kind, attempt = "task_queued", 0
            else:
                continue
            lines.append(
                {
                    "surface": "herdr_pane",
                    "agent": ASSIGNEE,
                    "task_id": task,
                    "message_key": f"atm:{message.group(1)}",
                    "kind": kind,
                    "attempt": attempt,
                    "terminal": block,
                }
            )
    return lines


def mailbox_terminal_lines(
    container: str, tasks: list[str]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    lines = []
    commands = []
    while True:
        result = run_cli(
            container,
            ASSIGNER,
            ["read", "--unread", "--json", "--no-since-last-seen"],
        )
        commands.append(result)
        if len(commands) > 32:
            raise RuntimeError("assigner mailbox did not drain within 32 reads")
        payload = json_value(result, "assigner mailbox read")
        if payload.get("count") == 0:
            break
        message = payload.get("message", {})
        task = message.get("taskId")
        if task not in tasks:
            continue
        operation = message.get("taskOp", {}).get("op")
        kind = {"start": "task_started", "close": "task_complete"}.get(operation)
        if kind is None:
            raise RuntimeError(f"unexpected task mailbox operation: {operation}")
        lines.append(
            {
                "surface": "atm_mailbox",
                "agent": ASSIGNER,
                "task_id": task,
                "message_key": f"atm:{message['message_id']}",
                "kind": kind,
                "attempt": 0,
                "terminal": message,
            }
        )
    return lines, commands


def handoff_identities(
    rows: list[list[Any]],
) -> Counter[tuple[str, str, str, str, int]]:
    return Counter((row[0], row[2], row[4], row[1], row[3]) for row in rows)


def terminal_identities(
    lines: list[dict[str, Any]],
) -> Counter[tuple[str, str, str, str, int]]:
    return Counter(
        (
            line["agent"],
            line["task_id"],
            line["message_key"],
            line["kind"],
            line["attempt"],
        )
        for line in lines
    )


def case_record(
    name: str,
    passed: bool,
    detail: str,
    commands: list[dict[str, Any]],
    observed: dict[str, Any],
    tasks: list[str],
) -> dict[str, Any]:
    return {
        "name": name,
        "status": "PASS" if passed else "FAIL",
        "detail": detail,
        "task_ids": tasks,
        "commands": commands,
        "observed": observed,
    }


def scenario_task_events(container: str, run_id: str) -> dict[str, Any]:
    tasks = [task_id(run_id, suffix) for suffix in ("events-one", "events-two", "events-three")]
    commands = [assign(container, task, f"BB6 prompt handoff {index}") for index, task in enumerate(tasks, 1)]
    wait_for_pane(
        container,
        lambda text: all(blocks_for(text, task) for task in tasks)
        and any(" ready" in block for block in blocks_for(text, tasks[0])),
        "three queued prompts and task one ready",
    )
    pane = wait_for_pane(
        container,
        lambda text: any('reminder="1"' in block for block in blocks_for(text, tasks[0])),
        "task one reminder attempt one",
    )
    start = run_cli(
        container,
        ASSIGNEE,
        ["task", "start", tasks[0], "--team", TEAM, "BB6 prompted task started"],
    )
    commands.append(start)
    event_results = [task_events(container, task) for task in tasks]
    commands.extend(result for result, _ in event_results)
    payloads = [payload for _, payload in event_results]
    kinds = [handoff_kinds(payload) for payload in payloads]
    events = [event_names(payload) for payload in payloads]
    passed = (
        all(command["exit_code"] == 0 for command in commands)
        and kinds[0]
        == ["task_queued", "task_ready", "task_reminder", "task_started"]
        and kinds[1:] == [["task_queued"], ["task_queued"]]
        and "started" in events[0]
        and all("started" not in names for names in events[1:])
    )
    return case_record(
        SCENARIO_NAMES[0],
        passed,
        "task one alone has ready, reminder, and started evidence",
        commands,
        {
            "handoff_kinds_by_task": dict(zip(tasks, kinds)),
            "event_names_by_task": dict(zip(tasks, events)),
            "task_blocks": {task: blocks_for(pane["stdout"], task) for task in tasks},
            "task_event_payloads": dict(zip(tasks, payloads)),
        },
        tasks,
    )


def release_first_scenario(container: str, tasks: list[str]) -> list[dict[str, Any]]:
    outcomes = []
    for task in tasks[1:]:
        outcomes.append(
            run_cli(
                container,
                ASSIGNEE,
                ["task", "close", task, "refused", "--team", TEAM, "BB6 runner release"],
            )
        )
    outcomes.append(
        run_cli(
            container,
            ASSIGNEE,
            ["task", "close", tasks[0], "completed", "--team", TEAM, "BB6 runner release"],
        )
    )
    if any(result["exit_code"] != 0 for result in outcomes):
        raise RuntimeError("failed to release the first scenario tasks")
    return outcomes


def scenario_disabled_reminder(container: str, run_id: str) -> dict[str, Any]:
    task = task_id(run_id, "disabled-reminder")
    disable = run_cli(
        container,
        ASSIGNER,
        ["teams", "disable-nudge-template", "--team", TEAM, "--kind", "task_reminder", "--json"],
    )
    assignment = assign(container, task, "BB6 disabled reminder")
    ready = wait_for_pane(
        container,
        lambda text: any(" ready" in block for block in blocks_for(text, task)),
        "disabled-reminder task ready",
    )
    time.sleep(REMINDER_WAIT_SECONDS)
    after = pane_text(container)
    event_result, payload = task_events(container, task)
    doctor = run_cli(container, ASSIGNER, ["doctor", "--json", "--team", TEAM])
    doctor_payload = json_value(doctor, "doctor")
    findings = doctor_payload.get("findings", []) if isinstance(doctor_payload, dict) else []
    messages = [row.get("message", "") for row in findings if isinstance(row, dict)]
    blocks = blocks_for(after["stdout"], task) if after["exit_code"] == 0 else []
    kinds = handoff_kinds(payload)
    passed = (
        all(result["exit_code"] == 0 for result in (disable, assignment, ready, after, event_result, doctor))
        and kinds == ["task_queued", "task_ready"]
        and all("reminder=" not in block for block in blocks)
        and any("disabled_task_nudge_template_override" in message for message in messages)
    )
    return case_record(
        SCENARIO_NAMES[1],
        passed,
        "disabled task_reminder emits no reminder handoff and doctor reports the override",
        [disable, assignment, ready, after, event_result, doctor],
        {
            "handoff_kinds": kinds,
            "task_blocks": blocks,
            "doctor_findings": findings,
            "wait_seconds": REMINDER_WAIT_SECONDS,
            "task_event_payload": payload,
        },
        [task],
    )


def inspect_container(container: str) -> dict[str, Any]:
    result = run_command(["docker", "inspect", container])
    payload = json_value(result, "docker inspect")
    record = payload[0]
    image_id = record.get("Image")
    image = run_command(["docker", "image", "inspect", image_id])
    image_record = json_value(image, "docker image inspect")[0]
    return {
        "container": container,
        "image_id": image_id,
        "image_sha256": image_id,
        "image_architecture": image_record.get("Architecture"),
        "image_repo_tags": image_record.get("RepoTags", []),
    }


def render_html(report: dict[str, Any]) -> str:
    rows = "".join(
        f'<tr class="{case["status"].lower()}"><td>{case["status"]}</td>'
        f'<td>{escape(case["name"])}</td><td>{escape(case["detail"])}</td></tr>'
        for case in report["cases"]
    )
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>BB.6 prompt-handoff colima evidence</title>
<style>body{{font:16px system-ui,sans-serif;max-width:80rem;margin:2rem auto;padding:0 1rem}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #ddd;padding:.5rem;text-align:left}}.pass{{color:#176b2c}}.fail{{color:#a00}}pre{{white-space:pre-wrap;max-height:30rem;overflow:auto;background:#f5f5f5;padding:1rem}}</style></head>
<body><h1>BB.6 prompt-handoff colima evidence</h1>
<p>Status: <strong>{escape(report['status'])}</strong>; source revision: <code>{escape(report['source_revision'])}</code></p>
<p>Container: <code>{escape(report['container'])}</code>; image: <code>{escape(report['image']['image_id'])}</code></p>
<p>Acceptance count: handoffs={report['acceptance']['prompt_handoffs_count']}, task-linked terminal lines={report['acceptance']['task_linked_terminal_line_count']}, record failures={report['acceptance']['prompt_handoff_record_failed_count']}.</p>
<table><thead><tr><th>Status</th><th>Scenario</th><th>Expectation</th></tr></thead><tbody>{rows}</tbody></table>
<h2>Machine-readable transcript</h2><pre>{escape(json.dumps(report, indent=2))}</pre></body></html>"""


def run(container: str, out_dir: Path) -> int:
    generated_at = datetime.now(timezone.utc)
    run_id = generated_at.strftime("%Y%m%dT%H%M%S%fZ")
    source = run_command(["git", "rev-parse", "HEAD"])
    source_revision = source["stdout"].strip() if source["exit_code"] == 0 else "unknown"
    baseline_result = sqlite_count(container)
    baseline_count = integer_result(baseline_result, "baseline prompt_handoffs count")
    if baseline_count != 0:
        raise RuntimeError(f"fresh fixture required; prompt_handoffs starts at {baseline_count}")
    first = scenario_task_events(container, run_id)
    cleanup = release_first_scenario(container, first["task_ids"])
    second = scenario_disabled_reminder(container, run_id)
    final_pane = pane_text(container)
    if final_pane["exit_code"] != 0:
        raise RuntimeError("failed to read final tester pane")
    all_tasks = first["task_ids"] + second["task_ids"]
    terminal_lines = pane_terminal_lines(final_pane["stdout"], all_tasks)
    mailbox_lines, mailbox_commands = mailbox_terminal_lines(container, all_tasks)
    terminal_lines.extend(mailbox_lines)
    terminal_line_count = len(terminal_lines)
    count_result = sqlite_count(container)
    handoff_count = integer_result(count_result, "final prompt_handoffs count")
    rows_result = prompt_handoff_rows(container)
    rows = json_value(rows_result, "prompt handoff rows")
    if not isinstance(rows, list):
        raise RuntimeError("prompt handoff rows returned a non-list payload")
    log_result = prompt_failure_count(container)
    failure_count = integer_result(log_result, "prompt-handoff failure count")
    stored_identities = handoff_identities(rows)
    observed_identities = terminal_identities(terminal_lines)
    acceptance_passed = (
        handoff_count == terminal_line_count
        and stored_identities == observed_identities
        and failure_count == 0
    )
    report = {
        "feature": "bb6-prompt-handoffs",
        "generated_at": generated_at.isoformat().replace("+00:00", "Z"),
        "source_revision": source_revision,
        "container": container,
        "image": inspect_container(container),
        "cases": [first, second],
        "setup": {"baseline_prompt_handoffs": baseline_result},
        "cleanup_between_scenarios": cleanup,
        "assigner_mailbox_commands": mailbox_commands,
        "acceptance": {
            "status": "PASS" if acceptance_passed else "FAIL",
            "query": "SELECT COUNT(*) FROM prompt_handoffs",
            "prompt_handoffs_count": handoff_count,
            "task_linked_terminal_line_count": terminal_line_count,
            "prompt_handoff_record_failed_count": failure_count,
            "terminal_lines": terminal_lines,
            "stored_handoff_rows": rows,
            "identity_fields": [
                "agent",
                "task_id",
                "message_key",
                "kind",
                "attempt",
            ],
            "identities_match": stored_identities == observed_identities,
            "count_command": count_result,
            "rows_command": rows_result,
            "log_command": log_result,
        },
    }
    report["status"] = (
        "PASS"
        if all(case["status"] == "PASS" for case in report["cases"])
        and acceptance_passed
        else "FAIL"
    )
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "bb6-prompt-handoffs.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (out_dir / "index.html").write_text(render_html(report), encoding="utf-8")
    print(f"BB6 prompt-handoff colima: {report['status']}")
    for case in report["cases"]:
        print(f"{case['status']} {case['name']}")
    print(
        "acceptance: "
        f"handoffs={handoff_count} terminal_lines={terminal_line_count} "
        f"record_failures={failure_count}"
    )
    print(f"source revision: {source_revision}")
    print(f"image: {report['image']['image_id']}")
    print(f"evidence: {out_dir}")
    return 0 if report["status"] == "PASS" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", default=DEFAULT_CONTAINER)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    out_dir = args.out or (
        ROOT
        / "site/reports/bb6-prompt-handoffs"
        / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    )
    try:
        return run(args.container, out_dir.resolve())
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"BB6 prompt-handoff colima: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

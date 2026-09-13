#!/usr/bin/env python3
"""Run the BB.5 assignment/task-pass scenarios against a live colima fixture.

The fixture is driven only through the public ``atm`` CLI and Herdr's pane
observation API.  The complete command transcript is written as immutable
runner evidence under ``site/reports/bb5-assignment``.
"""

from __future__ import annotations

import argparse
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
DEFAULT_CONTAINER = "hermes-testbed-bb5"
ASSIGNER = "stub-alpha"
ASSIGNEE = "tester"
POLL_SECONDS = 0.5
WAIT_SECONDS = 90.0
SCENARIO_NAMES = (
    "three_assignments_show_queued_1_2_3_then_one_ready",
    "pending_ack_header_stays_zero_across_assignments",
    "close_shows_ready_for_next_task_within_one_pass",
    "reassign_shows_closed_reassigned_to_old_and_queued_to_new",
    "move_to_head_while_idle_shows_ready_next_pass_and_no_extra_line",
    "cancel_shows_closed_cancelled_to_assignee",
    "busy_assigner_receives_started_and_complete_at_write_time",
)


def run_command(command: list[str], timeout: float = 45.0) -> dict[str, Any]:
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    return {
        "command": " ".join(command),
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def run_cli(container: str, identity: str, args: list[str], timeout: float = 45.0) -> dict[str, Any]:
    return run_command([
        "docker", "exec", "-e", f"ATM_IDENTITY={identity}", "-e", f"ATM_TEAM={TEAM}",
        container, "atm", *args,
    ], timeout)


def run_herdr(container: str, args: list[str], timeout: float = 45.0) -> dict[str, Any]:
    return run_command(["docker", "exec", container, "herdr", *args], timeout)


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


def task_id(run_id: str, suffix: str, group: str) -> str:
    return f"BB5-{run_id[-14:]}-{group}-{suffix}"


def assign(container: str, identity: str, assignee: str, task: str, message: str) -> dict[str, Any]:
    return run_cli(container, identity, [
        "task", "assign", assignee, "--task-id", task, "--team", TEAM, message,
    ])


def close(container: str, identity: str, task: str, outcome: str = "cancelled") -> dict[str, Any]:
    return run_cli(container, identity, [
        "task", "close", task, outcome, "--team", TEAM, "BB5 runner cleanup",
    ])


def agent_pane(container: str) -> str:
    result = run_herdr(container, ["agent", "list"])
    if result["exit_code"] != 0:
        raise RuntimeError(f"herdr agent list failed: {result['stderr'].strip() or result['stdout'].strip()}")
    payload = json_value(result, "herdr agent list")
    agents = payload.get("result", {}).get("agents", []) if isinstance(payload, dict) else []
    pane = next((agent.get("pane_id") for agent in agents if agent.get("name") == ASSIGNEE), None)
    if not isinstance(pane, str):
        raise RuntimeError(f"no pane for {ASSIGNEE} in herdr agent list")
    return pane


def pane_text(container: str, lines: int = 500) -> dict[str, Any]:
    return run_herdr(container, ["pane", "read", agent_pane(container), "--source", "recent-unwrapped", "--lines", str(lines)])


def wait_for_pane(container: str, predicate: Callable[[str], bool], label: str) -> dict[str, Any]:
    deadline = time.monotonic() + WAIT_SECONDS
    last = pane_text(container)
    while time.monotonic() < deadline:
        if last["exit_code"] == 0 and predicate(last["stdout"]):
            return last
        time.sleep(POLL_SECONDS)
        last = pane_text(container)
    raise RuntimeError(f"timed out waiting for {label}; last pane output: {last['stdout'][-2000:]}")


def blocks_for(text: str, task: str) -> list[str]:
    pattern = re.compile(rf'<atm\b[^>]*\btask="{re.escape(task)}"[\s\S]*?(?:</atm>|/>)')
    return pattern.findall(text)


def drain_mailbox(container: str, identity: str) -> list[dict[str, Any]]:
    reads: list[dict[str, Any]] = []
    for _ in range(40):
        result = run_cli(container, identity, ["read", "--team", TEAM, "--json"])
        reads.append(result)
        if result["exit_code"] != 0:
            break
        payload = json_value(result, "mailbox drain")
        counts = payload.get("bucket_counts", {}) if isinstance(payload, dict) else {}
        if counts.get("unread", 0) == 0:
            break
    run_cli(container, identity, ["clear", "--team", TEAM])
    return reads


def cleanup_tasks(container: str, tasks: list[str], identities: tuple[str, ...] = (ASSIGNER,)) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for task in dict.fromkeys(tasks):
        result = close(container, identities[0], task)
        results.append({"task_id": task, "result": result})
    drain_mailbox(container, ASSIGNEE)
    drain_mailbox(container, "stub-beta")
    return results


def case_record(name: str, status: str, detail: str, commands: list[dict[str, Any]], observed: dict[str, Any], tasks: list[str]) -> dict[str, Any]:
    return {
        "name": name, "status": status, "detail": detail, "origin": "stub-alpha@testbed",
        "destination": "tester@testbed", "attempt": 1, "task_ids": tasks,
        "commands": commands, "observed": observed,
    }


def scenario_three_assignments(container: str, run_id: str) -> dict[str, Any]:
    tasks = [task_id(run_id, suffix, "queued") for suffix in ("one", "two", "three")]
    commands = [assign(container, ASSIGNER, ASSIGNEE, task, f"BB5 queued task {index}") for index, task in enumerate(tasks, 1)]
    pane = wait_for_pane(container, lambda text: all(
        blocks_for(text, task) and any(
            f'queued="{index}"' in block for block in blocks_for(text, task)
        ) for index, task in enumerate(tasks, 1)
    ) and blocks_for(text, tasks[0]) and " ready" in blocks_for(text, tasks[0])[-1], "three assignment queue and ready lines")
    blocks = {task: blocks_for(pane["stdout"], task) for task in tasks}
    ready_counts = {task: sum(" ready" in block for block in task_blocks) for task, task_blocks in blocks.items()}
    queued_positions = {
        task: [
            int(match.group(1))
            for block in block_list
            for match in [re.search(r'queued="(\d+)"', block)]
            if match
        ]
        for task, block_list in blocks.items()
    }
    passed = (
        all(result["exit_code"] == 0 for result in commands)
        and all(ready_counts[task] == (1 if task == tasks[0] else 0) for task in tasks)
        and all("execute" not in "\n".join(blocks[task]) for task in tasks[1:])
    )
    cleanup_tasks(container, tasks)
    return case_record(SCENARIO_NAMES[0], "PASS" if passed else "FAIL", "three queued positions and exactly one ready prompt for task one", commands, {"blocks": blocks, "ready_counts": ready_counts, "queued_positions": queued_positions}, tasks)


def scenario_pending_ack(container: str, run_id: str) -> dict[str, Any]:
    tasks = [task_id(run_id, suffix, "pending") for suffix in ("one", "two", "three")]
    commands = [assign(container, ASSIGNER, ASSIGNEE, task, "BB5 pending-ack header") for task in tasks]
    read = run_cli(container, ASSIGNEE, ["read", "--team", TEAM])
    header = "\n".join(read["stdout"].splitlines()[:3])
    passed = all(result["exit_code"] == 0 for result in commands) and read["exit_code"] == 0 and "Unread: 3" in header and "Pending-Ack: 0" in header
    cleanup_tasks(container, tasks)
    return case_record(SCENARIO_NAMES[1], "PASS" if passed else "FAIL", "assignment mailbox header is Unread 3 / Pending-Ack 0", commands + [read], {"header": header}, tasks)


def scenario_close_ready(container: str, run_id: str) -> dict[str, Any]:
    tasks = [task_id(run_id, suffix, "close") for suffix in ("one", "two")]
    commands = [assign(container, ASSIGNER, ASSIGNEE, task, "BB5 close readiness") for task in tasks]
    wait_for_pane(container, lambda text: blocks_for(text, tasks[0]) and " ready" in blocks_for(text, tasks[0])[-1], "first ready prompt")
    finish = run_cli(container, ASSIGNEE, ["task", "close", tasks[0], "completed", "--team", TEAM, "BB5 completed first task"])
    commands.append(finish)
    pane = wait_for_pane(container, lambda text: blocks_for(text, tasks[1]) and " ready" in blocks_for(text, tasks[1])[-1], "next task ready after close")
    block = blocks_for(pane["stdout"], tasks[1])[-1]
    passed = all(result["exit_code"] == 0 for result in commands) and block.count(" ready") == 1
    cleanup_tasks(container, [tasks[1]])
    drain_mailbox(container, ASSIGNER)
    return case_record(SCENARIO_NAMES[2], "PASS" if passed else "FAIL", "closing task one yields one ready prompt for task two", commands, {"next_task_block": block}, tasks)


def scenario_reassign(container: str, run_id: str) -> dict[str, Any]:
    task = task_id(run_id, "reassign", "reassign")
    first = assign(container, ASSIGNER, ASSIGNEE, task, "BB5 reassignment")
    wait_for_pane(container, lambda text: blocks_for(text, task) and 'queued="1"' in blocks_for(text, task)[-1], "old assignee queue line")
    reassigned = run_cli(container, ASSIGNER, ["task", "assign", "stub-beta", "--task-id", task, "--team", TEAM, "BB5 moved to stub-beta"])
    pane = wait_for_pane(container, lambda text: blocks_for(text, task) and 'outcome="reassigned"' in "\n".join(blocks_for(text, task)), "reassigned close line")
    listing = run_cli(container, ASSIGNER, ["task", "list", "--all", "--json"])
    rows = task_rows(json_value(listing, "reassignment task list")) if listing["exit_code"] == 0 else []
    row = next((item for item in rows if item.get("task_id") == task), {})
    block = "\n".join(blocks_for(pane["stdout"], task))
    passed = first["exit_code"] == reassigned["exit_code"] == listing["exit_code"] == 0 and 'outcome="reassigned"' in block and row.get("assignee") == "stub-beta" and row.get("position") == 1
    cleanup_tasks(container, [task])
    return case_record(SCENARIO_NAMES[3], "PASS" if passed else "FAIL", "old assignee receives closed=reassigned and new assignee owns queued position one", [first, reassigned, listing], {"old_assignee_block": block, "new_assignee_row": row}, [task])


def scenario_move_head(container: str, run_id: str) -> dict[str, Any]:
    tasks = [task_id(run_id, suffix, "move") for suffix in ("one", "two", "three")]
    commands = [assign(container, ASSIGNER, ASSIGNEE, task, "BB5 move-to-head") for task in tasks]
    wait_for_pane(container, lambda text: all(
        blocks_for(text, task) and any(
            f'queued="{index}"' in block for block in blocks_for(text, task)
        ) for index, task in enumerate(tasks, 1)
    ), "move-to-head queue lines")
    move = run_cli(container, ASSIGNER, ["task", "move", "--head", "--team", TEAM, tasks[2]])
    commands.append(move)
    pane = wait_for_pane(container, lambda text: blocks_for(text, tasks[2]) and " ready" in blocks_for(text, tasks[2])[-1], "moved head ready prompt")
    blocks = {task: "\n".join(blocks_for(pane["stdout"], task)) for task in tasks}
    passed = (
        move["exit_code"] == 0
        and 'outcome="' not in blocks[tasks[2]]
        and blocks[tasks[2]].count(" ready") == 1
        and blocks[tasks[0]].count(" ready") <= 1
        and " ready" not in blocks[tasks[1]]
    )
    cleanup_tasks(container, tasks)
    return case_record(SCENARIO_NAMES[4], "PASS" if passed else "FAIL", "moving task three to head while busy yields its single ready line on the next idle pass", commands, {"blocks": blocks}, tasks)


def scenario_cancel(container: str, run_id: str) -> dict[str, Any]:
    task = task_id(run_id, "cancel", "cancel")
    assignment = assign(container, ASSIGNER, ASSIGNEE, task, "BB5 cancellation")
    cancelled = close(container, ASSIGNER, task)
    pane = wait_for_pane(container, lambda text: blocks_for(text, task) and 'outcome="cancelled"' in "\n".join(blocks_for(text, task)), "cancelled close line")
    block = "\n".join(blocks_for(pane["stdout"], task))
    passed = assignment["exit_code"] == cancelled["exit_code"] == 0 and 'outcome="cancelled"' in block
    drain_mailbox(container, ASSIGNEE)
    return case_record(SCENARIO_NAMES[5], "PASS" if passed else "FAIL", "assignee receives the cancelled terminal line", [assignment, cancelled], {"block": block}, [task])


def scenario_busy_assigner(container: str, run_id: str) -> dict[str, Any]:
    task = task_id(run_id, "busy", "busy")
    busy_task = task_id(run_id, "assigner-active", "busy")
    busy_assignment = run_cli(container, ASSIGNEE, [
        "task", "assign", ASSIGNER, "--task-id", busy_task, "--team", TEAM,
        "BB5 active task keeps assigner busy",
    ])
    busy_start = run_cli(container, ASSIGNER, [
        "task", "start", busy_task, "--team", TEAM, "BB5 assigner became busy",
    ])
    assignment = assign(container, ASSIGNER, ASSIGNEE, task, "BB5 busy assigner")
    started = run_cli(container, ASSIGNEE, ["task", "start", task, "--team", TEAM, "BB5 started while assigner is busy"])
    completed = run_cli(container, ASSIGNEE, ["task", "close", task, "completed", "--team", TEAM, "BB5 completed while assigner is busy"])
    mailbox = run_cli(container, ASSIGNER, ["list", "--json", "--team", TEAM])
    messages = json_value(mailbox, "busy assigner mailbox") if mailbox["exit_code"] == 0 else []
    rows = [row for row in task_rows(messages) if row.get("task_id") == task]
    reads = [
        run_cli(container, ASSIGNER, [
            "read", "--message-id", row["message_id"], "--team", TEAM, "--json",
        ])
        for row in rows if row.get("message_id")
    ]
    operations = []
    for read in reads:
        if read["exit_code"] == 0:
            message = json_value(read, "busy assigner notice").get("message", {})
            operation = message.get("taskOp", message.get("task_op"))
            if isinstance(operation, dict):
                operations.append(operation)
    observed_ops = sorted(op.get("op") for op in operations)
    passed = all(result["exit_code"] == 0 for result in (busy_assignment, busy_start, assignment, started, completed, mailbox, *reads)) and observed_ops == ["close", "start"]
    cleanup_tasks(container, [busy_task])
    drain_mailbox(container, ASSIGNER)
    return case_record(SCENARIO_NAMES[6], "PASS" if passed else "FAIL", "busy assigner mailbox contains start and complete notices immediately after writes", [busy_assignment, busy_start, assignment, started, completed, mailbox, *reads], {"messages": rows, "observed_operations": observed_ops}, [busy_task, task])


def inspect_container(container: str) -> dict[str, Any]:
    result = run_command(["docker", "inspect", container])
    if result["exit_code"] != 0:
        raise RuntimeError(result["stderr"].strip() or "docker inspect failed")
    record = json.loads(result["stdout"])[0]
    image_id = record.get("Image")
    image_tag = record.get("Config", {}).get("Image")
    image = run_command(["docker", "image", "inspect", image_tag or image_id])
    image_record = json.loads(image["stdout"])[0] if image["exit_code"] == 0 else {}
    repo_digests = image_record.get("RepoDigests", [])
    return {
        "container": container,
        "image_tag": image_tag,
        "image_id": image_id,
        "image_sha256": image_id,
        "image_architecture": image_record.get("Architecture"),
        "image_repo_digests": repo_digests,
    }


def render_html(report: dict[str, Any]) -> str:
    rows = "".join(
        f"<tr class=\"{case['status'].lower()}\"><td>{'✓' if case['status'] == 'PASS' else '✗'}</td>"
        f"<td>{escape(case['name'])}</td><td>{escape(case['detail'])}</td><td>{case['status']}</td></tr>"
        for case in report["cases"]
    )
    return f"""<!doctype html>
<html lang=\"en\"><head><meta charset=\"utf-8\"><title>BB.5 assignment/task-pass colima evidence</title>
<style>body{{font:16px system-ui,sans-serif;max-width:80rem;margin:2rem auto;padding:0 1rem}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #ddd;padding:.5rem;text-align:left}}.pass{{color:#176b2c}}.fail{{color:#a00}}pre{{white-space:pre-wrap;max-height:30rem;overflow:auto;background:#f5f5f5;padding:1rem}}</style></head>
<body><h1>BB.5 assignment/task-pass colima evidence</h1>
<p>Status: <strong>{escape(report['status'])}</strong>; source revision: <code>{escape(report['source_revision'])}</code></p>
<p>Container: <code>{escape(report['container'])}</code>; image: <code>{escape(report['image'].get('image_id', 'unknown'))}</code>; roster: <code>{escape(', '.join(report['roster']))}</code></p>
<table><thead><tr><th>Status</th><th>Scenario</th><th>Expectation</th><th>Result</th></tr></thead><tbody>{rows}</tbody></table>
<h2>Machine-readable transcript</h2><pre>{escape(json.dumps(report, indent=2))}</pre></body></html>"""


def run(container: str, out_dir: Path) -> int:
    generated_at = datetime.now(timezone.utc)
    run_id = generated_at.strftime("%Y%m%dT%H%M%S%fZ")
    source = run_command(["git", "rev-parse", "HEAD"])
    source_revision = source["stdout"].strip() if source["exit_code"] == 0 else "unknown"
    image = inspect_container(container)
    version = run_cli(container, ASSIGNER, ["--version"])
    members = run_cli(container, ASSIGNER, ["members", "--json", "--team", TEAM])
    payload = json_value(members, "member roster")
    roster = [member.get("name") for member in payload.get("members", []) if isinstance(member, dict) and member.get("name")]
    expected_roster = {"hermes", "oversight", "stub-alpha", "stub-beta", "tester"}
    if set(roster) != expected_roster:
        raise RuntimeError(f"unexpected fixture roster: {roster!r}")
    report: dict[str, Any] = {
        "feature": "bb5-assignment", "generated_at": generated_at.isoformat().replace("+00:00", "Z"),
        "source_revision": source_revision, "container": container, "image": image,
        "image_sha256": image.get("image_sha256"),
        "atm_version": version,
        "roster": roster, "cases": [], "setup": {"members": members},
    }
    scenarios = (
        scenario_three_assignments, scenario_pending_ack, scenario_close_ready,
        scenario_reassign, scenario_move_head, scenario_cancel, scenario_busy_assigner,
    )
    for scenario, name in zip(scenarios, SCENARIO_NAMES):
        try:
            report["cases"].append(scenario(container, run_id))
        except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
            report["cases"].append(case_record(name, "FAIL", str(error), [], {}, []))
    report["status"] = "PASS" if len(report["cases"]) == 7 and all(case["status"] == "PASS" for case in report["cases"]) else "FAIL"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "bb5-assignment.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    (out_dir / "index.html").write_text(render_html(report), encoding="utf-8")
    print(f"BB5 assignment/task-pass colima: {report['status']}")
    for case in report["cases"]:
        print(f"{case['status']} {case['name']}")
    print(f"source revision: {source_revision}")
    print(f"image: {image.get('image_id', 'unknown')}")
    print(f"evidence: {out_dir}")
    return 0 if report["status"] == "PASS" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", default=DEFAULT_CONTAINER, help="already-running rebuilt fixture container")
    parser.add_argument("--out", type=Path, help="evidence directory; defaults under site/reports/bb5-assignment")
    args = parser.parse_args()
    out_dir = args.out or ROOT / "site/reports" / "bb5-assignment" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    try:
        return run(args.container, out_dir.resolve())
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"BB5 assignment/task-pass colima: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run the ordered colima integration sequence in one Hermes testbed container."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.integration import render_colima  # noqa: E402
from scripts.smoke import colima_skill_report  # noqa: E402
from scripts.smoke import run_bb4_task_start  # noqa: E402
from scripts.smoke import run_bb5_assignment  # noqa: E402
from scripts.smoke import run_bb6_prompt_handoffs  # noqa: E402

DEFAULT_TESTBED = Path.home() / "Documents/github/atm-hermes-testbed"
DEFAULT_CONTAINER = "hermes-testbed"
STEP_ORDER = ("hermes-skills", "task-start", "assignment", "prompt-handoffs")
FEATURES = {
    "hermes-skills": "colima-hermes-skills",
    "task-start": "bb4-task-start",
    "assignment": "bb5-assignment",
    "prompt-handoffs": "bb6-prompt-handoffs",
}
RESULTS_LINE = re.compile(r"^reports and logs:\s+(.+)$", re.MULTILINE)
StepRunner = Callable[[str, Path], dict[str, Any]]
FixtureReset = Callable[[str], dict[str, Any]]

RESET_POLICY_PATH = ROOT / "scripts/integration/colima_reset_tables.json"
RESET_POLICY = json.loads(RESET_POLICY_PATH.read_text(encoding="utf-8"))
CLEAR_TABLES = tuple(RESET_POLICY["clear"])
MANAGED_CLEAR_TABLES = tuple(RESET_POLICY["clear_via_owner"])
PRESERVED_TABLES = tuple(RESET_POLICY["keep"])
RESET_SQL = "\n".join(
    (
        "PRAGMA foreign_keys = ON;",
        "BEGIN IMMEDIATE;",
        *(f"DELETE FROM {table};" for table in CLEAR_TABLES),
        "COMMIT;",
        "PRAGMA wal_checkpoint(TRUNCATE);",
    )
)


def reset_command() -> str:
    database_reset = f"""
import json
import sqlite3

connection = sqlite3.connect('/root/.atm/db/mail.db')
kept = {PRESERVED_TABLES!r}
classified = set({CLEAR_TABLES!r}) | set({MANAGED_CLEAR_TABLES!r}) | set(kept)
actual = {{row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")}}
if actual != classified:
    raise SystemExit(f'unclassified migrated tables: {{sorted(actual ^ classified)}}')
before = {{table: connection.execute(f'SELECT * FROM {{table}} ORDER BY 1').fetchall() for table in kept}}
connection.executescript({RESET_SQL!r})
after = {{table: connection.execute(f'SELECT * FROM {{table}} ORDER BY 1').fetchall() for table in kept}}
if not before['peer_local_certificate'] or after != before:
    raise SystemExit('reset changed required peer, roster, or interface metadata')
print('reset preserved metadata: ' + json.dumps({{table: len(rows) for table, rows in after.items()}}, sort_keys=True))
""".strip()
    return rf"""
set -eu
herdr pane list 2>/dev/null | python3 -c '
import json, subprocess, sys
for pane in json.load(sys.stdin)["result"]["panes"]:
    subprocess.run(["herdr", "pane", "close", pane["pane_id"]], capture_output=True)
' 2>/dev/null || true
pkill -f '[h]ermes gateway run' 2>/dev/null || true
pkill -f '[h]erdr server' 2>/dev/null || true
pkill -x atm-daemon 2>/dev/null || true
for unused in $(seq 1 20); do
  pgrep -x atm-daemon >/dev/null 2>&1 || break
  sleep 1
done
rm -f /root/.atm/daemon/owner.lock /root/.atm/daemon/local-http.json
mkdir -p /root/.atm/logs
: > /root/.atm/logs/atm.log.jsonl
python3 -c {shlex.quote(database_reset)}
/opt/testbed/harness/bringup.sh
""".strip()


class DriverError(RuntimeError):
    """The fixture could not be started or a requested step was invalid."""


def command_record(command: list[str], *, cwd: Path | None = None, timeout: float = 1800.0) -> dict[str, Any]:
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False)
    return {
        "command": " ".join(command),
        "exit_code": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }


def parse_steps(value: str | None) -> tuple[str, ...]:
    if value is None:
        return STEP_ORDER
    requested = [item.strip() for item in value.split(",") if item.strip()]
    unknown = sorted(set(requested) - set(STEP_ORDER))
    if unknown:
        raise DriverError(f"unknown step(s): {', '.join(unknown)}; known: {', '.join(STEP_ORDER)}")
    if len(requested) != len(set(requested)):
        raise DriverError("each step may be selected only once")
    selected = tuple(name for name in STEP_ORDER if name in requested)
    if not selected:
        raise DriverError("--steps must select at least one step")
    return selected


def container_running(container: str) -> bool:
    result = command_record(["docker", "inspect", "--format", "{{.State.Running}}", container], timeout=30.0)
    return result["exit_code"] == 0 and result["stdout"].strip() == "true"


def start_fixture(testbed: Path, container: str) -> dict[str, Any]:
    if container != DEFAULT_CONTAINER:
        raise DriverError(f"the testbed run script owns container name {DEFAULT_CONTAINER!r}; got {container!r}")
    if container_running(container):
        return {"status": "reused", "container": container}
    result = command_record([str(testbed / "run.sh"), "--gateway", "--no-peer"], cwd=testbed)
    if result["exit_code"] != 0 or not container_running(container):
        detail = result["stderr"].strip() or result["stdout"].strip()
        raise DriverError(f"testbed start failed: {detail}")
    return {"status": "started", "container": container, "command": result}


def reset_fixture(container: str) -> dict[str, Any]:
    """Reset mutable ATM/team state while retaining the one container session."""
    result = command_record(["docker", "exec", container, "sh", "-lc", reset_command()])
    if result["exit_code"] != 0:
        detail = result["stderr"].strip() or result["stdout"].strip()
        raise DriverError(f"fixture state reset failed: {detail}")
    return result


def run_skills(testbed: Path, step_dir: Path) -> dict[str, Any]:
    """Let test.sh build/start the fixture once and convert its raw run into this step."""
    result = command_record([str(testbed / "test.sh")], cwd=testbed, timeout=3600.0)
    match = RESULTS_LINE.search(result["stdout"])
    if match is None:
        raise DriverError(
            "testbed skill run did not report its results directory: "
            + (result["stderr"].strip() or result["stdout"].strip())
        )
    raw_run = Path(match.group(1).strip())
    payload = colima_skill_report.run_step(raw_run, step_dir)
    payload["testbed_command"] = result
    (step_dir / "step.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return payload


def failure_payload(name: str, container: str, error: BaseException) -> dict[str, Any]:
    source = command_record(["git", "rev-parse", "HEAD"], cwd=ROOT, timeout=30.0)
    return {
        "feature": FEATURES[name],
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "source_revision": source["stdout"].strip() if source["exit_code"] == 0 else None,
        "container": container,
        "status": "FAIL",
        "cases": [{"name": f"{name}_step_execution", "status": "FAIL", "detail": str(error)}],
    }


def write_failure(step_dir: Path, payload: dict[str, Any]) -> None:
    step_dir.mkdir(parents=True, exist_ok=True)
    (step_dir / "step.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def step_runners(testbed: Path) -> dict[str, StepRunner]:
    return {
        "hermes-skills": lambda _container, out: run_skills(testbed, out),
        "task-start": run_bb4_task_start.run_step,
        "assignment": run_bb5_assignment.run_step,
        "prompt-handoffs": run_bb6_prompt_handoffs.run_step,
    }


def run_sequence(
    run_dir: Path,
    selected: tuple[str, ...],
    *,
    testbed: Path,
    container: str,
    runners: dict[str, StepRunner] | None = None,
    fixture_start: Callable[[Path, str], dict[str, Any]] = start_fixture,
    fixture_reset: FixtureReset = reset_fixture,
) -> dict[str, Any]:
    active_runners = runners or step_runners(testbed)
    fixture_ready = False
    for order, name in enumerate(selected, start=1):
        step_dir = run_dir / "steps" / f"{order:02d}-{name}"
        reset_record: dict[str, Any] | None = None
        try:
            if name == "hermes-skills":
                payload = active_runners[name](container, step_dir)
                fixture_ready = container_running(container)
            else:
                if not fixture_ready:
                    fixture_start(testbed, container)
                    fixture_ready = True
                reset_record = fixture_reset(container)
                payload = active_runners[name](container, step_dir)
            if payload.get("status") not in render_colima.VERDICTS:
                raise DriverError(f"{name} returned no PASS/FAIL status")
        except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
            payload = failure_payload(name, container, error)
            if not fixture_ready:
                try:
                    fixture_start(testbed, container)
                    fixture_ready = True
                except (OSError, RuntimeError, subprocess.SubprocessError, ValueError):
                    pass
        if reset_record is not None:
            payload["fixture_reset"] = reset_record
        write_failure(step_dir, payload)
        render_colima.add_step(run_dir, name, step_dir / "step.json", order)
    return render_colima.render_run(run_dir, root=ROOT)


def default_run_dir() -> Path:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return ROOT / "site/reports/integration/colima" / stamp


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", nargs="?", choices=("colima",), default="colima", help=argparse.SUPPRESS)
    parser.add_argument("--out", type=Path, help="run directory (default: site/reports/integration/colima/<timestamp>)")
    parser.add_argument("--steps", help=f"comma-separated subset of: {','.join(STEP_ORDER)}")
    parser.add_argument("--testbed", type=Path, default=DEFAULT_TESTBED, help="atm-hermes-testbed checkout")
    parser.add_argument("--container", default=DEFAULT_CONTAINER, help="fixture container name")
    args = parser.parse_args(argv[1:])
    try:
        selected = parse_steps(args.steps)
        testbed = args.testbed.expanduser().resolve()
        if not (testbed / "test.sh").is_file() or not (testbed / "run.sh").is_file():
            raise DriverError(f"not an atm-hermes-testbed checkout: {testbed}")
        run_dir = (args.out or default_run_dir()).resolve()
        summary = run_sequence(run_dir, selected, testbed=testbed, container=args.container)
    except (DriverError, render_colima.RenderError) as error:
        print(f"colima integration: error: {error}", file=sys.stderr)
        return 2
    print(f"colima integration: {summary['status']} ({len(summary['steps'])} step(s))")
    print(f"evidence: {run_dir}")
    return 0 if summary["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

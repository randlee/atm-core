#!/usr/bin/env python3
"""Run ATM task-lifecycle prompts in one disposable Hermes testbed fixture."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.integration import render_colima  # noqa: E402
from scripts.smoke import colima_skill_report  # noqa: E402

DEFAULT_TESTBED = Path.home() / "Documents/github/atm-hermes-testbed"
DEFAULT_CONTAINER = "hermes-testbed"
PROMPTS = (
    ("AT9", "task-start", "AT9-task-start"),
    ("AT10", "assignment", "AT10-assignment"),
    ("AT11", "prompt-handoffs", "AT11-prompt-handoffs"),
)
PROMPT_SCHEMA = "prompt-report-1"
VERDICTS = {"pass": "PASS", "fail": "FAIL"}
RESULTS_LINE = re.compile(r"^reports and logs:\s+(.+)$", re.MULTILINE)
Command = Callable[[list[str], Path | None, float], dict[str, Any]]
SkillRunner = Callable[[Path, Path, Command], dict[str, Any]]


class DriverError(RuntimeError):
    """The fixture could not run or returned an invalid prompt report."""


class FixtureBringupError(DriverError):
    """The canonical testbed entry point could not bring up its fixture."""


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def command_record(command: list[str], cwd: Path | None = None, timeout: float = 1800.0) -> dict[str, Any]:
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False)
    return {"command": " ".join(command), "exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def source_revision(root: Path) -> str | None:
    result = command_record(["git", "rev-parse", "HEAD"], root, 30.0)
    return result["stdout"].strip() if result["exit_code"] == 0 else None


def validate_prompt_report(report: Any, expected_test_id: str) -> dict[str, Any]:
    if not isinstance(report, dict):
        raise DriverError("prompt report must be a JSON object")
    required = {"schema", "test_id", "agent", "steps", "verdict", "atm_versions", "started_at", "finished_at"}
    missing = sorted(required - report.keys())
    if missing:
        raise DriverError(f"prompt report missing fields: {', '.join(missing)}")
    if report["schema"] != PROMPT_SCHEMA or report["test_id"] != expected_test_id:
        raise DriverError(
            f"prompt report identity mismatch: expected {PROMPT_SCHEMA}/{expected_test_id}, "
            f"got {report['schema']!r}/{report['test_id']!r}"
        )
    if report["verdict"] not in VERDICTS:
        raise DriverError("prompt report verdict must be pass or fail")
    if not isinstance(report["steps"], list) or not report["steps"]:
        raise DriverError("prompt report steps must be a non-empty list")
    for index, step in enumerate(report["steps"], start=1):
        if not isinstance(step, dict) or set(step) != {"name", "status", "detail"}:
            raise DriverError(f"prompt report step {index} must contain name, status, detail")
        if step["status"] not in {"pass", "fail", "skip"}:
            raise DriverError(f"prompt report step {index} has invalid status {step['status']!r}")
    return report


def step_payload(report: dict[str, Any], *, step_name: str, revision: str | None, container: str) -> dict[str, Any]:
    cases = [
        {
            "name": str(item["name"]),
            "status": {"pass": "PASS", "fail": "FAIL", "skip": "SKIP"}[item["status"]],
            "detail": str(item["detail"]),
            "prompt_status": item["status"],
        }
        for item in report["steps"]
    ]
    return {
        "feature": f"colima-{step_name}",
        "generated_at": report["finished_at"],
        "source_revision": revision,
        "container": container,
        "atm_version": report["atm_versions"].get("atm") if isinstance(report["atm_versions"], dict) else None,
        "status": VERDICTS[report["verdict"]],
        "cases": cases,
        "prompt_report": report,
    }


def failure_payload(step_name: str, revision: str | None, container: str, error: BaseException) -> dict[str, Any]:
    return {
        "feature": f"colima-{step_name}",
        "generated_at": now(),
        "source_revision": revision,
        "container": container,
        "status": "FAIL",
        "cases": [{"name": "prompt execution", "status": "FAIL", "detail": str(error)}],
    }


def run_hermes_skills(testbed: Path, step_dir: Path, run_command: Command = command_record) -> dict[str, Any]:
    """Run the testbed's canonical entry point and convert its retained reports."""
    execution = run_command([str(testbed / "test.sh")], testbed, 3600.0)
    match = RESULTS_LINE.search(execution["stdout"])
    if match is None:
        detail = execution["stderr"].strip() or execution["stdout"].strip() or "test.sh reported no result directory"
        bringup_markers = ("cannot run", "run.sh failed")
        error_type = FixtureBringupError if any(marker in detail.lower() for marker in bringup_markers) else DriverError
        raise error_type(detail)
    payload = colima_skill_report.run_step(Path(match.group(1).strip()), step_dir)
    payload["testbed_execution"] = execution
    return payload


def run_prompt(
    prompt_id: str,
    step_name: str,
    expected_test_id: str,
    step_dir: Path,
    *,
    container: str,
    revision: str | None,
    run_command: Command = command_record,
) -> dict[str, Any]:
    execution = run_command(["docker", "exec", container, "/opt/testbed/harness/run-prompts.sh", prompt_id], None, 1200.0)
    if execution["exit_code"] != 0:
        raise DriverError(execution["stderr"].strip() or execution["stdout"].strip() or f"{prompt_id} failed")
    step_dir.mkdir(parents=True, exist_ok=True)
    report_path = step_dir / f"prompt-{prompt_id}.json"
    copied = run_command(["docker", "cp", f"{container}:/opt/testbed/results/prompt-{prompt_id}.json", str(report_path)], None, 60.0)
    if copied["exit_code"] != 0:
        raise DriverError(copied["stderr"].strip() or f"missing prompt report for {prompt_id}")
    try:
        report = validate_prompt_report(json.loads(report_path.read_text(encoding="utf-8")), expected_test_id)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DriverError(f"invalid {prompt_id} report: {error}") from error
    payload = step_payload(report, step_name=step_name, revision=revision, container=container)
    payload["prompt_execution"] = execution
    return payload


def run_sequence(
    run_dir: Path,
    *,
    testbed: Path,
    container: str = DEFAULT_CONTAINER,
    root: Path = ROOT,
    run_command: Command = command_record,
    skill_runner: SkillRunner = run_hermes_skills,
    renderer: Callable[..., dict[str, Any]] = render_colima.render_run,
) -> dict[str, Any]:
    revision = source_revision(root)
    skills_dir = run_dir / "steps/01-hermes-skills"
    bringup_failed = False
    try:
        skills_payload = skill_runner(testbed, skills_dir, run_command)
    except FixtureBringupError as error:
        bringup_failed = True
        skills_payload = failure_payload("hermes-skills", revision, container, error)
    except (DriverError, OSError, subprocess.SubprocessError, ValueError) as error:
        skills_payload = failure_payload("hermes-skills", revision, container, error)
    skills_dir.mkdir(parents=True, exist_ok=True)
    (skills_dir / "step.json").write_text(json.dumps(skills_payload, indent=2) + "\n", encoding="utf-8")
    for order, (prompt_id, step_name, test_id) in enumerate(PROMPTS, start=2):
        step_dir = run_dir / "steps" / f"{order:02d}-{step_name}"
        try:
            if bringup_failed:
                raise DriverError("not run: fixture bringup failed (step 1)")
            payload = run_prompt(
                prompt_id,
                step_name,
                test_id,
                step_dir,
                container=container,
                revision=revision,
                run_command=run_command,
            )
        except (DriverError, OSError, subprocess.SubprocessError, ValueError) as error:
            payload = failure_payload(step_name, revision, container, error)
        step_dir.mkdir(parents=True, exist_ok=True)
        (step_dir / "step.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return renderer(run_dir, root=root)


def default_run_dir() -> Path:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return ROOT / "site/reports/integration/colima" / stamp


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", nargs="?", choices=("colima",), default="colima", help=argparse.SUPPRESS)
    parser.add_argument("--out", type=Path, help="run directory under site/reports/integration/colima")
    parser.add_argument("--testbed", type=Path, default=DEFAULT_TESTBED)
    parser.add_argument("--container", default=DEFAULT_CONTAINER)
    args = parser.parse_args(argv[1:])
    testbed = args.testbed.expanduser().resolve()
    if not (testbed / "test.sh").is_file():
        print(f"colima integration: error: not an atm-hermes-testbed checkout: {testbed}", file=sys.stderr)
        return 2
    run_dir = (args.out or default_run_dir()).resolve()
    try:
        summary = run_sequence(run_dir, testbed=testbed, container=args.container)
    except (DriverError, render_colima.RenderError, OSError) as error:
        print(f"colima integration: error: {error}", file=sys.stderr)
        return 2
    print(f"colima integration: {summary['status']} ({len(summary['steps'])} step(s))")
    print(f"evidence: {run_dir}")
    return 0 if summary["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

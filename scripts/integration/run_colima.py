#!/usr/bin/env python3
"""Run the canonical Hermes fixture and its ATM lifecycle prompts."""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from scripts.integration import render_colima  # noqa: E402
from scripts.smoke import colima_skill_report  # noqa: E402

CONTAINER = "hermes-testbed"
DEFAULT_TESTBED = Path.home() / "Documents/github/atm-hermes-testbed"
PROMPTS = (("AT9", "task-start"), ("AT10", "assignment"), ("AT11", "prompt-handoffs"))
RESULTS_LINE = re.compile(r"^reports and logs:\s+(.+)$", re.MULTILINE)


def command(argv: list[str], cwd: Path | None = None, timeout: float = 1800) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False)


def write_step(path: Path, payload: dict[str, object]) -> None:
    path.mkdir(parents=True, exist_ok=True)
    (path / "step.json").write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def failed(step: str, revision: str) -> dict[str, object]:
    return {
        "feature": f"colima-{step}",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "source_revision": revision,
        "container": CONTAINER,
        "status": "FAIL",
        "cases": [{"name": "report", "status": "FAIL", "detail": "report missing; see run-prompts.log"}],
    }


def run_prompt(prompt: str, step: str, step_dir: Path, revision: str) -> dict[str, object]:
    ran = command(["docker", "exec", CONTAINER, "/opt/testbed/harness/run-prompts.sh", prompt], timeout=1200)
    step_dir.mkdir(parents=True, exist_ok=True)
    (step_dir / "run-prompts.log").write_text(ran.stdout + ran.stderr, encoding="utf-8")
    report_path = step_dir / f"prompt-{prompt}.json"
    copied = command(
        ["docker", "cp", f"{CONTAINER}:/opt/testbed/results/prompt-{prompt}.json", str(report_path)], timeout=60
    )
    if copied.returncode:
        with (step_dir / "run-prompts.log").open("a", encoding="utf-8") as log:
            log.write(copied.stdout + copied.stderr)
        return failed(step, revision)
    report = json.loads(report_path.read_text(encoding="utf-8"))
    return {
        "feature": f"colima-{step}",
        "generated_at": report["finished_at"],
        "source_revision": revision,
        "container": CONTAINER,
        "atm_version": report["atm_versions"].get("atm"),
        "status": report["verdict"].upper(),
        "cases": report["steps"],
    }


def run(run_dir: Path, testbed: Path) -> dict[str, object]:
    revision = command(["git", "rev-parse", "HEAD"], ROOT, 30).stdout.strip()
    fixture = command([str(testbed / "test.sh")], testbed, 3600)
    first = run_dir / "steps/01-hermes-skills"
    first.mkdir(parents=True, exist_ok=True)
    (first / "test.sh.log").write_text(fixture.stdout + fixture.stderr, encoding="utf-8")
    match = RESULTS_LINE.search(fixture.stdout)
    if match:
        colima_skill_report.run_step(Path(match.group(1).strip()), first)
    else:
        write_step(first, failed("hermes-skills", revision))
    for order, (prompt, step) in enumerate(PROMPTS, 2):
        step_dir = run_dir / "steps" / f"{order:02d}-{step}"
        write_step(step_dir, run_prompt(prompt, step, step_dir, revision))
    return render_colima.render_run(run_dir, root=ROOT)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", nargs="?", choices=("colima",), default="colima", help=argparse.SUPPRESS)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--testbed", type=Path, default=DEFAULT_TESTBED)
    args = parser.parse_args()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    run_dir = (args.out or ROOT / "site/reports/integration/colima" / stamp).resolve()
    summary = run(run_dir, args.testbed.expanduser().resolve())
    print(f"colima integration: {summary['status']} ({len(summary['steps'])} step(s))")
    print(f"evidence: {run_dir}")
    return 0 if summary["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())

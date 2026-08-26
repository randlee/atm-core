"""Run the required benchmark and preserve its verdict across shell hosts."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> int:
    """Run one repository command from the checkout root."""
    return subprocess.run(command, cwd=ROOT, check=False).returncode


def main() -> int:
    """Retain a measured runner failure while still rebuilding its reports."""
    runner_status = run([sys.executable, "scripts/smoke/run_admission_capacity.py", *sys.argv[1:]])
    report_status = run([sys.executable, "scripts/smoke/benchmark_report.py", "--rebuild"])
    if report_status:
        return report_status
    index_status = run([sys.executable, ".just/generate_report_index.py", "--check"])
    if index_status:
        return index_status
    print("View the newest campaign panel: just benchmark-show")
    return runner_status


if __name__ == "__main__":
    raise SystemExit(main())

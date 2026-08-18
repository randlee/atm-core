from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


SCRIPT = (
    Path(__file__).resolve().parents[1] / "check_boundary_lint_status.py"
)


def run_status_check(report_path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(report_path)],
        capture_output=True,
        check=False,
        text=True,
    )


def test_boundary_lint_status_check_passes_only_a_pass_report(tmp_path: Path) -> None:
    report_path = tmp_path / "boundary-lint.json"
    report_path.write_text(json.dumps({"status": "pass", "findings": []}), encoding="utf-8")

    result = run_status_check(report_path)

    assert result.returncode == 0


def test_boundary_lint_status_check_fails_a_report_with_findings(tmp_path: Path) -> None:
    report_path = tmp_path / "boundary-lint.json"
    report_path.write_text(
        json.dumps({"status": "fail", "findings": [{"rule_id": "SCB-CYCLE-001"}]}),
        encoding="utf-8",
    )

    result = run_status_check(report_path)

    assert result.returncode == 1


def test_boundary_lint_status_check_rejects_a_malformed_status(tmp_path: Path) -> None:
    report_path = tmp_path / "boundary-lint.json"
    report_path.write_text(json.dumps({"status": "unknown"}), encoding="utf-8")

    result = run_status_check(report_path)

    assert result.returncode == 2
    assert "invalid status" in result.stderr

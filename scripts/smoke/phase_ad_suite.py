#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
import json
import shlex
import subprocess
import sys
import tempfile
import time

from fixtures import current_binary_sha, repo_root


@dataclass(frozen=True)
class SuiteRowSpec:
    id: str
    flow: str
    commands: list[list[str]]
    pass_note: str


@dataclass
class SmokeRow:
    id: str
    flow: str
    verdict: str = "SKIP"
    notes: str = ""
    observed_behavior: str | None = None
    expected_behavior: str | None = None
    likely_root_cause: str | None = None
    artifact_pointer: str | None = None

    def to_payload(self) -> dict[str, object]:
        payload = asdict(self)
        return {key: value for key, value in payload.items() if value is not None}


def run_suite(level: str, specs: list[SuiteRowSpec], *, write_artifacts: bool) -> dict[str, object]:
    root = repo_root()
    started = time.perf_counter()
    rows: list[SmokeRow] = []
    for spec in specs:
        rows.append(run_row(root, spec))
    payload = {
        "level": level,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "binary_sha": current_binary_sha(root),
        "duration_secs": round(time.perf_counter() - started, 3),
        "status": "passed" if all(row.verdict == "PASS" for row in rows) else "failed",
        "rows": [row.to_payload() for row in rows],
        "summary": {
            "pass": sum(1 for row in rows if row.verdict == "PASS"),
            "fail": sum(1 for row in rows if row.verdict == "FAIL"),
            "skip": sum(1 for row in rows if row.verdict == "SKIP"),
        },
    }
    render_payload(payload, write_artifacts=write_artifacts)
    return payload


def run_row(root: Path, spec: SuiteRowSpec) -> SmokeRow:
    row = SmokeRow(id=spec.id, flow=spec.flow)
    for command in spec.commands:
        completed = subprocess.run(
            command,
            cwd=root,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if completed.returncode != 0:
            row.verdict = "FAIL"
            row.notes = f"command failed: {shlex.join(command)}"
            row.observed_behavior = build_failure_observation(completed)
            row.expected_behavior = "all targeted validation commands exit 0"
            row.likely_root_cause = "one or more targeted AD.11 evidence checks regressed"
            row.artifact_pointer = shlex.join(command)
            return row
    row.verdict = "PASS"
    row.notes = spec.pass_note
    return row


def build_failure_observation(completed: subprocess.CompletedProcess[str]) -> str:
    payload = {
        "command": completed.args,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }
    return json.dumps(payload, indent=2)


def render_payload(payload: dict[str, object], *, write_artifacts: bool) -> None:
    root = repo_root()
    with tempfile.NamedTemporaryFile(
        "w",
        encoding="utf-8",
        prefix="phase-ad-smoke-",
        suffix=".json",
        delete=False,
    ) as handle:
        temp_path = Path(handle.name)
        json.dump(payload, handle, indent=2)
        handle.write("\n")
    try:
        command = [
            sys.executable,
            str(root / "scripts" / "smoke" / "render_report.py"),
            str(temp_path),
        ]
        if write_artifacts:
            command.append("--write-artifacts")
        subprocess.run(command, cwd=root, check=True)
    finally:
        temp_path.unlink(missing_ok=True)

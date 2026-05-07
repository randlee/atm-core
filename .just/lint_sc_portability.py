#!/usr/bin/env python3
from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import argparse
import json
import subprocess
import sys
import time

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import print_report
from lint_common import workspace_crate_section_lines


def command(repo_root: Path) -> list[str]:
    return [
        "cargo",
        "run",
        "-q",
        "-p",
        "sc-lint-boundary",
        "--",
        "analyze",
        "--root",
        str(repo_root),
        "--rule",
        "portability",
        "--format",
        "json",
    ]


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    start_time = time.perf_counter()
    cmd = command(repo_root)
    result = subprocess.run(
        cmd,
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    duration_seconds = time.perf_counter() - start_time

    transcript = [
        *workspace_crate_section_lines(repo_root),
        f"command: {' '.join(cmd)}",
        f"exit_code: {result.returncode}",
        "",
        "stdout:",
        result.stdout.rstrip(),
        "",
        "stderr:",
        result.stderr.rstrip(),
    ]

    if result.returncode != 0:
        report = build_report(
            lint_name="sc-portability",
            repo_root=repo_root,
            passed=False,
            summary="sc-lint-portability execution failed",
            findings=[result.stderr.strip() or result.stdout.strip() or "sc-lint-portability exited non-zero"],
            transcript_lines=transcript,
            started_at=started_at,
            duration_seconds=duration_seconds,
        )
        print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
        return 1

    payload = json.loads(result.stdout)
    findings = [finding["message"] for finding in payload.get("findings", [])]
    status = payload.get("status")
    passed = status == "pass"
    summary = (
        f"sc-lint-portability status={status} findings={len(findings)}"
        if status is not None
        else f"sc-lint-portability findings={len(findings)}"
    )
    report = build_report(
        lint_name="sc-portability",
        repo_root=repo_root,
        passed=passed,
        summary=summary,
        findings=findings,
        transcript_lines=transcript,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
    return 0 if passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run sc-lint portability checks as a preliminary repo lint.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

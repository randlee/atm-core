#!/usr/bin/env python3
from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import argparse
import subprocess
import sys
import time

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import render_workspace_crate_table
from lint_common import workspace_crates
from lint_common import workspace_target_args


def module_cycle_command(repo_root: Path, manifest_path: Path, package_name: str) -> list[str]:
    return [
        "cargo",
        "modules",
        "dependencies",
        "--package",
        package_name,
        "--manifest-path",
        str(manifest_path),
        *workspace_target_args(manifest_path),
        "--no-fns",
        "--no-sysroot",
        "--acyclic",
        "--layout",
        "none",
    ]


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    start_time = time.perf_counter()
    transcript = [
        "crates analyzed:",
        *render_workspace_crate_table(repo_root),
        "",
    ]
    findings: list[str] = []

    for crate in workspace_crates(repo_root):
        manifest_path = repo_root / crate.manifest_path
        command = module_cycle_command(repo_root, manifest_path, crate.package_name)
        result = subprocess.run(
            command,
            cwd=repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        transcript.extend(
            [
                f"crate: {crate.crate_dir}",
                f"command: {' '.join(command)}",
                f"exit_code: {result.returncode}",
            ]
        )
        if result.stdout.strip():
            transcript.extend(["stdout:", result.stdout.rstrip()])
        if result.stderr.strip():
            transcript.extend(["stderr:", result.stderr.rstrip()])
        transcript.append("")

        if result.returncode != 0:
            detail = result.stderr.strip() or result.stdout.strip() or "cargo-modules reported a cycle"
            findings.append(f"{crate.crate_dir}: {detail}")

    duration_seconds = time.perf_counter() - start_time
    if findings:
        report = build_report(
            lint_name="modules",
            repo_root=repo_root,
            passed=False,
            summary=f"module dependency cycle check failed for {len(findings)} crate(s)",
            findings=findings,
            transcript_lines=transcript,
            started_at=started_at,
            duration_seconds=duration_seconds,
        )
        print("module cycle check failed.")
        for finding in report.findings[:3]:
            print(finding)
        print(f"errors: {len(report.findings)}")
        return 1

    build_report(
        lint_name="modules",
        repo_root=repo_root,
        passed=True,
        summary="module dependency graphs are acyclic",
        findings=[],
        transcript_lines=transcript,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print("module cycle check passed: cargo-modules found no internal dependency cycles.")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Check workspace crates for internal module dependency cycles.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import is_code_line
from lint_common import iter_workspace_rust_files
from lint_common import load_lint_config
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines
from datetime import datetime, timezone


LINT_NAME = "fixed-sleep"
SLEEP_RE = re.compile(r"\b(?:(?:std::)?thread|tokio::time)::sleep\s*\(")


@dataclass(frozen=True)
class FixedSleepViolation:
    path: str
    line_number: int
    line: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: {self.line}"


def load_allowed_paths(repo_root: Path) -> tuple[str, ...]:
    config = load_lint_config(repo_root).get("fixed_sleep", {})
    if not isinstance(config, dict):
        raise SystemExit("[fixed_sleep] must be a TOML table")
    allowed_paths = config.get("allowed_paths", [])
    # Empty is intentional: ordinary test code has no blanket exemptions. Only
    # narrow daemon-runtime suites may be added here after explicit review.
    if not isinstance(allowed_paths, list) or not all(isinstance(item, str) for item in allowed_paths):
        raise SystemExit("[fixed_sleep].allowed_paths must be an array of path strings")
    return tuple(allowed_paths)


def collect_fixed_sleep_violations(
    repo_root: Path,
    *,
    allowed_paths: tuple[str, ...],
) -> list[FixedSleepViolation]:
    violations: list[FixedSleepViolation] = []
    allowed_path_set = set(allowed_paths)
    for abs_path in iter_workspace_rust_files(repo_root):
        rel_path = abs_path.relative_to(repo_root)
        rel_posix = rel_path.as_posix()
        lines = abs_path.read_text(encoding="utf-8").splitlines()
        scope = rust_file_test_scope(rel_path, lines)

        for line_number, (line, in_test_scope) in enumerate(zip(lines, scope, strict=True), start=1):
            if not in_test_scope:
                continue
            if rel_posix in allowed_path_set:
                continue
            if not is_code_line(line):
                continue
            if not SLEEP_RE.search(line):
                continue
            violations.append(
                FixedSleepViolation(
                    path=rel_posix,
                    line_number=line_number,
                    line=line.strip(),
                )
            )
    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Reject fixed thread::sleep(...) in ordinary Rust test code.")
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    violations = collect_fixed_sleep_violations(
        repo_root,
        allowed_paths=load_allowed_paths(repo_root),
    )
    duration_seconds = monotonic_now() - start_time

    transcript_lines = [
        *workspace_crate_section_lines(repo_root),
        "findings:",
        *([violation.render() for violation in violations] or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "fixed thread::sleep(...) usage found in Rust test scope"
            if violations
            else "no fixed thread::sleep(...) usage found in Rust test scope"
        ),
        findings=[violation.render() for violation in violations],
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=3, direct_threshold=3)
    return 0 if report.passed else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

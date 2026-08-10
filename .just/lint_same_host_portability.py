#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines


LINT_NAME = "same-host-portability"
SHARED_HOST_SHELL_FILES = (
    "crates/atm/src/composition.rs",
)
ADAPTER_FILES = (
    "crates/atm-daemon/src/lifecycle_control.rs",
    "crates/atm-daemon/src/host_ownership.rs",
)
UNIX_GATING_RE = re.compile(r"#\[\s*cfg\s*\((?:all|any)?\s*\(?\s*unix\b")
NON_UNIX_STUB_RE = re.compile(r"#\[\s*cfg\s*\(\s*not\s*\(\s*unix\s*\)\s*\)\s*\]")
DAEMON_UNAVAILABLE_RE = re.compile(r"\bdaemon_unavailable\s*\(")
LEGACY_PORTABILITY_TODO_RE = re.compile(r"TODO\(S\.2/ADR-007\)")


@dataclass(frozen=True)
class Violation:
    path: str
    line_number: int
    message: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: {self.message}"


def iter_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8").splitlines()


def collect_shared_host_shell_gating(repo_root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for rel_path in SHARED_HOST_SHELL_FILES:
        abs_path = repo_root / rel_path
        lines = iter_lines(abs_path)
        test_scope = rust_file_test_scope(Path(rel_path), lines)
        for line_number, line in enumerate(lines, start=1):
            if test_scope[line_number - 1]:
                continue
            if UNIX_GATING_RE.search(line):
                violations.append(
                    Violation(
                        path=rel_path,
                        line_number=line_number,
                        message=(
                            "broad #[cfg(unix)] gating is not allowed above the owned same-host "
                            "portability adapter layer"
                        ),
                    )
                )
    return violations


def collect_non_unix_same_host_stubs(repo_root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for rel_path in ADAPTER_FILES:
        abs_path = repo_root / rel_path
        lines = iter_lines(abs_path)
        test_scope = rust_file_test_scope(Path(rel_path), lines)
        for line_number, line in enumerate(lines, start=1):
            if test_scope[line_number - 1]:
                continue
            if not NON_UNIX_STUB_RE.search(line):
                continue
            window = lines[line_number : min(line_number + 8, len(lines))]
            for offset, candidate in enumerate(window, start=1):
                if test_scope[min(line_number - 1 + offset, len(test_scope) - 1)]:
                    continue
                if DAEMON_UNAVAILABLE_RE.search(candidate):
                    violations.append(
                        Violation(
                            path=rel_path,
                            line_number=line_number + offset,
                            message=(
                                "non-Unix same-host production paths must not fall back to "
                                "daemon_unavailable(...) stubs"
                            ),
                        )
                    )
                    break
    return violations


def collect_forbidden_todo_markers(repo_root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for rel_path in ADAPTER_FILES:
        abs_path = repo_root / rel_path
        lines = iter_lines(abs_path)
        test_scope = rust_file_test_scope(Path(rel_path), lines)
        for line_number, line in enumerate(lines, start=1):
            if test_scope[line_number - 1]:
                continue
            if LEGACY_PORTABILITY_TODO_RE.search(line):
                violations.append(
                    Violation(
                        path=rel_path,
                        line_number=line_number,
                        message=(
                            "Phase S closeout must not leave TODO(S.2/ADR-007) portability "
                            "markers in production same-host adapter code"
                        ),
                    )
                )
    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Reject broad Unix-only same-host gating above the adapter layer and "
            "non-Unix same-host daemon_unavailable stubs in adapter modules."
        )
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    violations = [
        *collect_shared_host_shell_gating(repo_root),
        *collect_non_unix_same_host_stubs(repo_root),
        *collect_forbidden_todo_markers(repo_root),
    ]
    duration_seconds = monotonic_now() - start_time

    transcript_lines = [
        *workspace_crate_section_lines(repo_root),
        "same_host_shell_files:",
        *SHARED_HOST_SHELL_FILES,
        "",
        "adapter_files:",
        *ADAPTER_FILES,
        "",
        "findings:",
        *([violation.render() for violation in violations] or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "same-host portability regressions found"
            if violations
            else "no same-host portability regressions found"
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

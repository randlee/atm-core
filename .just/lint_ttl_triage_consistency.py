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


LINT_NAME = "ttl-triage"
STATE_RE = re.compile(
    r"triage:(?P<field>status|findingAggregate|branchStatus|branchR[A-Za-z0-9_.-]+Status)\s+"
    r"(?:\"(?P<quoted>[^\"]+)\"|triage:(?P<qualified>[A-Za-z0-9_.-]+))"
)


@dataclass(frozen=True)
class TriageConsistencyViolation:
    path: str
    line_number: int
    message: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: {self.message}"


def iter_ttl_files(repo_root: Path) -> list[Path]:
    triage_root = repo_root / ".triage"
    if not triage_root.exists():
        return []
    return sorted(triage_root.rglob("*.ttl"))


def extract_finding_block(text: str) -> tuple[list[str], int] | None:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if "a triage:Finding" not in line:
            continue
        start = index
        while start > 0 and lines[start - 1].strip():
            start -= 1
        end = index + 1
        while end < len(lines) and lines[end].strip():
            end += 1
        return lines[start:end], start + 1
    return None


def normalize_state(match: re.Match[str]) -> str:
    value = match.group("quoted") or match.group("qualified") or ""
    return value.strip().lower()


def collect_ttl_triage_violations(repo_root: Path) -> list[TriageConsistencyViolation]:
    violations: list[TriageConsistencyViolation] = []
    for ttl_path in iter_ttl_files(repo_root):
        rel_path = ttl_path.relative_to(repo_root).as_posix()
        finding_block = extract_finding_block(ttl_path.read_text(encoding="utf-8"))
        if finding_block is None:
            continue
        lines, start_line = finding_block
        top_statuses: list[tuple[str, int]] = []
        aggregate_statuses: list[tuple[str, int]] = []
        branch_statuses: list[tuple[str, int]] = []

        for offset, line in enumerate(lines, start=start_line):
            for match in STATE_RE.finditer(line):
                state = normalize_state(match)
                field = match.group("field")
                if field == "status":
                    top_statuses.append((state, offset))
                elif field == "findingAggregate":
                    aggregate_statuses.append((state, offset))
                else:
                    branch_statuses.append((state, offset))

        if top_statuses and aggregate_statuses:
            top_state, top_line = top_statuses[0]
            aggregate_state, aggregate_line = aggregate_statuses[0]
            if top_state != aggregate_state:
                violations.append(
                    TriageConsistencyViolation(
                        path=rel_path,
                        line_number=aggregate_line,
                        message=(
                            f"contradictory triage status fields: status={top_state} "
                            f"but findingAggregate={aggregate_state}"
                        ),
                    )
                )

        resolved_state = None
        resolved_line = None
        if aggregate_statuses:
            resolved_state, resolved_line = aggregate_statuses[0]
        elif top_statuses:
            resolved_state, resolved_line = top_statuses[0]
        if resolved_state in {"fixed", "absent"}:
            for branch_state, branch_line in branch_statuses:
                if branch_state == "open":
                    violations.append(
                        TriageConsistencyViolation(
                            path=rel_path,
                            line_number=branch_line,
                            message=(
                                f"contradictory triage branch state: aggregate={resolved_state} "
                                "but branch status remains open"
                            ),
                        )
                    )
                    break
    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Reject contradictory aggregate and branch status state in triage Turtle records."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    violations = collect_ttl_triage_violations(repo_root)
    duration_seconds = monotonic_now() - start_time

    transcript_lines = [
        "triage files analyzed:",
        *([path.relative_to(repo_root).as_posix() for path in iter_ttl_files(repo_root)] or ["none"]),
        "",
        "findings:",
        *([violation.render() for violation in violations] or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "contradictory triage Turtle state found"
            if violations
            else "no contradictory triage Turtle state found"
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

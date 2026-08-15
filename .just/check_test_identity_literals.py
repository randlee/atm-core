#!/usr/bin/env python3
from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import re
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import iter_string_literal_contents
from lint_common import iter_workspace_rust_files
from lint_common import is_code_line
from lint_common import load_lint_config
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines


LINT_NAME = "identities"
CONCAT_START_RE = re.compile(r"\bconcat!\s*\(")


@dataclass(frozen=True)
class IdentityViolation:
    path: str
    line_number: int
    line: str
    kind: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: {self.line}"


def load_forbidden_literals(repo_root: Path) -> tuple[str, ...]:
    config = load_lint_config(repo_root).get("identities", {})
    if not isinstance(config, dict):
        raise SystemExit("[identities] must be a TOML table")
    literals = config.get("forbidden_literals")
    if not isinstance(literals, list) or not all(isinstance(item, str) for item in literals):
        raise SystemExit("[identities].forbidden_literals must be an array of strings")
    return tuple(literals)


def load_production_canonical_literals(repo_root: Path) -> dict[str, tuple[str, ...]]:
    config = load_lint_config(repo_root).get("identities", {})
    if not isinstance(config, dict):
        raise SystemExit("[identities] must be a TOML table")
    literal_map = config.get("production_canonical_literals", {})
    if not isinstance(literal_map, dict):
        raise SystemExit("[identities].production_canonical_literals must be a TOML table")

    canonical_literals: dict[str, tuple[str, ...]] = {}
    for literal, allowed_paths in literal_map.items():
        if not isinstance(literal, str):
            raise SystemExit("[identities].production_canonical_literals keys must be strings")
        if not isinstance(allowed_paths, list) or not all(isinstance(item, str) for item in allowed_paths):
            raise SystemExit(
                "[identities].production_canonical_literals values must be arrays of path strings"
            )
        canonical_literals[literal] = tuple(allowed_paths)
    return canonical_literals


def _skip_rust_string(source: str, index: int) -> int:
    """Return the first character after the Rust string at ``index``."""
    if source[index] == '"':
        index += 1
        while index < len(source):
            if source[index] == "\\":
                index += 2
            elif source[index] == '"':
                return index + 1
            else:
                index += 1
        return len(source)

    raw_start = index
    if source[index] == "r":
        index += 1
        while index < len(source) and source[index] == "#":
            index += 1
        if index < len(source) and source[index] == '"':
            hashes = source[raw_start + 1 : index]
            closing = '"' + hashes
            end = source.find(closing, index + 1)
            return len(source) if end == -1 else end + len(closing)
    return raw_start + 1


def iter_multiline_concat_literals(lines: list[str]) -> list[tuple[int, str]]:
    """Find ``concat!`` calls and return their source line and joined literals.

    The existing per-line literal scan cannot see a forbidden identity split
    across lines. This deliberately reconstructs only literal fragments inside
    balanced ``concat!`` parentheses; identifiers and expressions contribute no
    text and therefore cannot create a false identity match.
    """
    source = "\n".join(lines)
    matches: list[tuple[int, str]] = []
    for start in CONCAT_START_RE.finditer(source):
        opening = source.find("(", start.start(), start.end())
        depth = 1
        index = opening + 1
        while index < len(source) and depth:
            character = source[index]
            if character in ('"', "r"):
                next_index = _skip_rust_string(source, index)
                if next_index != index + 1:
                    index = next_index
                    continue
            if character == "(":
                depth += 1
            elif character == ")":
                depth -= 1
            index += 1
        if depth:
            continue
        fragment = source[opening + 1 : index - 1]
        literals = iter_string_literal_contents(fragment)
        if len(literals) < 2:
            continue
        line_number = source.count("\n", 0, start.start()) + 1
        matches.append((line_number, "".join(literals)))
    return matches


def collect_identity_violations(
    repo_root: Path,
    *,
    forbidden_literals: tuple[str, ...],
    production_canonical_literals: dict[str, tuple[str, ...]],
) -> list[IdentityViolation]:
    violations: list[IdentityViolation] = []
    for abs_path in iter_workspace_rust_files(repo_root):
        rel_path = abs_path.relative_to(repo_root)
        lines = abs_path.read_text(encoding="utf-8").splitlines()
        scope = rust_file_test_scope(rel_path, lines)
        concat_literals = iter_multiline_concat_literals(lines)
        concat_by_line: dict[int, list[str]] = {}
        for line_number, value in concat_literals:
            concat_by_line.setdefault(line_number, []).append(value)

        for line_number, (line, in_test_scope) in enumerate(zip(lines, scope, strict=True), start=1):
            if not is_code_line(line):
                continue
            literal_contents = set(iter_string_literal_contents(line))
            literal_contents.update(concat_by_line.get(line_number, ()))
            if in_test_scope:
                if not any(literal in literal_contents for literal in forbidden_literals):
                    continue
                violations.append(
                    IdentityViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        kind="test_scope_forbidden_literal",
                    )
                )
                continue

            handled_literals: set[str] = set()
            for literal, allowed_paths in production_canonical_literals.items():
                if literal not in literal_contents:
                    continue
                handled_literals.add(literal)
                if rel_path.as_posix() in allowed_paths:
                    continue
                violations.append(
                    IdentityViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        kind="production_scope_canonical_literal",
                    )
                )
            for literal in forbidden_literals:
                if literal not in literal_contents or literal in handled_literals:
                    continue
                violations.append(
                    IdentityViolation(
                        path=rel_path.as_posix(),
                        line_number=line_number,
                        line=line.strip(),
                        kind="production_scope_forbidden_literal",
                    )
                )
                break

    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Reject forbidden test literals and duplicated canonical production literals in Rust code."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    forbidden_literals = load_forbidden_literals(repo_root)
    production_canonical_literals = load_production_canonical_literals(repo_root)
    violations = collect_identity_violations(
        repo_root,
        forbidden_literals=forbidden_literals,
        production_canonical_literals=production_canonical_literals,
    )
    duration_seconds = monotonic_now() - start_time

    findings = [violation.render() for violation in violations]
    transcript_lines = [
        *workspace_crate_section_lines(repo_root),
        "findings:",
        *(findings or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "raw production literals found in Rust code"
            if violations
            else "no disallowed raw production literals found in Rust code"
        ),
        findings=findings,
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    if not report.passed:
        print("RULE-008/RULE-009 violation: raw production literals found in Rust code.")
        print("Use centralized reserved-role constants or canonical production definition sites.")
        for finding in report.findings:
            print(finding)
        print(f"total violations: {len(report.findings)}")
        print_report(report, repo_root=repo_root, preview_limit=0, direct_threshold=0)
        return 1

    print_report(report, repo_root=repo_root, preview_limit=0, direct_threshold=0)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

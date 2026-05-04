#!/usr/bin/env python3
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys

from lint_common import LintDirectivePolicy
from lint_common import classify_rust_test_scope
from lint_common import discover_repo_root
from lint_common import is_code_line
from lint_common import line_is_suppressed
from lint_common import load_lint_config
from lint_common import workspace_crate_section_lines


LINT_NAME = "identities"
DIRECTIVE_POLICY = LintDirectivePolicy(
    tool_key=LINT_NAME,
    aliases=("rule-008", "rule-009"),
)


@dataclass(frozen=True)
class IdentityViolation:
    path: str
    line_number: int
    line: str

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


def iter_rust_files(repo_root: Path) -> list[Path]:
    return sorted((repo_root / "crates").rglob("*.rs"))


def file_scope(path: Path, lines: list[str]) -> list[bool]:
    rel_posix = path.as_posix()
    if "/tests/" in rel_posix:
        return [True] * len(lines)
    if "/src/" in rel_posix:
        return classify_rust_test_scope(lines)
    return [False] * len(lines)


def collect_identity_violations(
    repo_root: Path,
    *,
    forbidden_literals: tuple[str, ...],
) -> list[IdentityViolation]:
    violations: list[IdentityViolation] = []
    for abs_path in iter_rust_files(repo_root):
        rel_path = abs_path.relative_to(repo_root)
        lines = abs_path.read_text(encoding="utf-8").splitlines()
        scope = file_scope(rel_path, lines)

        for line_number, (line, in_test_scope) in enumerate(zip(lines, scope, strict=True), start=1):
            if not in_test_scope:
                continue
            if not is_code_line(line):
                continue
            if not any(literal in line for literal in forbidden_literals):
                continue
            if line_is_suppressed(line_number, lines, DIRECTIVE_POLICY):
                continue
            violations.append(
                IdentityViolation(
                    path=rel_path.as_posix(),
                    line_number=line_number,
                    line=line.strip(),
                )
            )

    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check test and cfg(test) Rust code for forbidden production identity literals."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    forbidden_literals = load_forbidden_literals(repo_root)
    violations = collect_identity_violations(
        repo_root,
        forbidden_literals=forbidden_literals,
    )

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    if violations:
        print("RULE-008/RULE-009 violation: raw production literals found in test/cfg(test) Rust code.")
        print("Use test constants, centralized reserved-role constants, or explicit lint suppressions.")
        for violation in violations:
            print(violation.render())
        print(f"total violations: {len(violations)}")
        return 1

    print("RULE-008/RULE-009 check passed: no disallowed raw production literals found in test scope.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

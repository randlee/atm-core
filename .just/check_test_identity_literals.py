#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


FORBIDDEN_LITERALS = ("team-lead", "arch-ctm", "atm-dev", "quality-mgr")

ALLOWED_PATTERNS = (
    re.compile(r"\b(?:pub\s+)?const\s+(?:ROLE_TEAM_LEAD|TEST_[A-Z0-9_]+)\b"),
    re.compile(r"\b(?:ROLE_TEAM_LEAD|TEST_[A-Z0-9_]+)\b"),
    re.compile(r"\bATM_(?:TEAM|IDENTITY)\b"),
)

EXCLUDED_PATHS = {
    "crates/atm/tests/support/mod.rs",
}


def is_test_scope(path: Path, text: str) -> bool:
    return "/tests/" in path.as_posix()


def collect_test_lines(repo_root: Path) -> list[tuple[Path, int, str]]:
    crate_root = repo_root / "crates"
    results: list[tuple[Path, int, str]] = []
    for abs_path in sorted(crate_root.rglob("*.rs")):
        rel_path = abs_path.relative_to(repo_root)
        if rel_path.as_posix() in EXCLUDED_PATHS:
            continue
        text = abs_path.read_text(encoding="utf-8")
        if not is_test_scope(rel_path, text):
            continue
        for line_number, line in enumerate(text.splitlines(), start=1):
            results.append((rel_path, line_number, line))

    return results


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    failures: list[tuple[str, int, str]] = []
    scanned_lines = collect_test_lines(repo_root)
    for path, line_number, line in scanned_lines:
        if not any(literal in line for literal in FORBIDDEN_LITERALS):
            continue
        if any(pattern.search(line) for pattern in ALLOWED_PATTERNS):
            continue
        failures.append((path.as_posix(), line_number, line.strip()))

    if failures:
        print("RULE-008 violation: raw production identity literals found in test/cfg(test) code.")
        print(
            "Use approved constants or explicit ATM_TEAM/ATM_IDENTITY env-var tests instead."
        )
        for path, line_number, line in failures:
            print(f"{path}:{line_number}: {line}")
        print(f"total violations: {len(failures)}")
        return 1

    print("RULE-008 check passed: no disallowed raw production identity literals found in test/cfg(test) code.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

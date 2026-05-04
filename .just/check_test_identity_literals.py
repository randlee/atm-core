#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


FORBIDDEN_LITERALS = ("team-lead", "arch-ctm", "atm-dev", "quality-mgr")

ALLOW_NEXT_LINE_PATTERN = re.compile(r"rule-008:\s*allow-next-line\b", re.IGNORECASE)
ALLOW_BLOCK_START_PATTERN = re.compile(r"rule-008:\s*allow-start\b", re.IGNORECASE)
ALLOW_BLOCK_END_PATTERN = re.compile(r"rule-008:\s*allow-end\b", re.IGNORECASE)


def is_test_scope(path: Path, text: str) -> bool:
    return True


def collect_test_lines(repo_root: Path) -> list[tuple[Path, int, str]]:
    crate_root = repo_root / "crates"
    results: list[tuple[Path, int, str]] = []
    for abs_path in sorted(crate_root.rglob("*.rs")):
        rel_path = abs_path.relative_to(repo_root)
        text = abs_path.read_text(encoding="utf-8")
        if not is_test_scope(rel_path, text):
            continue
        for line_number, line in enumerate(text.splitlines(), start=1):
            results.append((rel_path, line_number, line))

    return results


def line_has_allow_next_line(line_number: int, lines: list[str]) -> bool:
    if line_number <= 1:
        return False
    index = line_number - 2
    while index >= 0:
        line = lines[index]
        if ALLOW_NEXT_LINE_PATTERN.search(line):
            return True
        if not is_comment_line(line):
            return False
        index -= 1
    return False


def line_is_inside_allow_block(line_number: int, lines: list[str]) -> bool:
    allow_depth = 0
    for line in lines[: line_number - 1]:
        if ALLOW_BLOCK_START_PATTERN.search(line):
            allow_depth += 1
        if ALLOW_BLOCK_END_PATTERN.search(line):
            allow_depth = max(0, allow_depth - 1)
    return allow_depth > 0


def is_comment_or_empty(line: str) -> bool:
    stripped = line.strip()
    if not stripped:
        return True
    return stripped.startswith(("//", "///", "//!", "/*", "*", "*/"))


def is_comment_line(line: str) -> bool:
    stripped = line.strip()
    return stripped.startswith(("//", "///", "//!", "/*", "*", "*/"))


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    failures: list[tuple[str, int, str]] = []
    scanned_lines = collect_test_lines(repo_root)
    cached_lines: dict[str, list[str]] = {}
    for path, line_number, line in scanned_lines:
        if is_comment_or_empty(line):
            continue
        if not any(literal in line for literal in FORBIDDEN_LITERALS):
            continue
        path_key = path.as_posix()
        if path_key not in cached_lines:
            cached_lines[path_key] = (repo_root / path).read_text(encoding="utf-8").splitlines()
        if line_has_allow_next_line(line_number, cached_lines[path_key]):
            continue
        if line_is_inside_allow_block(line_number, cached_lines[path_key]):
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

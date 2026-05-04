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


def is_test_scope(path: Path, text: str) -> bool:
    return "/tests/" in path.as_posix() or "#[cfg(test)]" in text or "mod tests" in text


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    crate_root = repo_root / "crates"
    failures: list[tuple[str, int, str]] = []

    for path in sorted(crate_root.rglob("*.rs")):
        rel = path.relative_to(repo_root)
        text = path.read_text(encoding="utf-8")
        if not is_test_scope(rel, text):
            continue

        for line_number, line in enumerate(text.splitlines(), start=1):
            if not any(literal in line for literal in FORBIDDEN_LITERALS):
                continue
            if any(pattern.search(line) for pattern in ALLOWED_PATTERNS):
                continue
            failures.append((rel.as_posix(), line_number, line.strip()))

    if failures:
        print("RULE-008 violation: raw production identity literals found in test/cfg(test) code.")
        print("Use approved constants or explicit ATM_TEAM/ATM_IDENTITY env-var tests instead.")
        for path, line_number, line in failures:
            print(f"{path}:{line_number}: {line}")
        print(f"total violations: {len(failures)}")
        return 1

    print("RULE-008 check passed: no disallowed raw production identity literals found.")
    return 0


if __name__ == "__main__":
    sys.exit(main())


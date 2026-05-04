#!/usr/bin/env python3
from __future__ import annotations

import sys
from pathlib import Path


MAX_NON_TEST_LINES = 1000


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    failures: list[tuple[str, int]] = []

    for path in sorted((repo_root / "crates").rglob("*.rs")):
        rel = path.relative_to(repo_root)
        rel_posix = rel.as_posix()
        if "/tests/" in rel_posix:
            continue
        if "/src/" not in rel_posix:
            continue

        line_count = sum(1 for _ in path.open("r", encoding="utf-8"))
        if line_count > MAX_NON_TEST_LINES:
            failures.append((rel_posix, line_count))

    if failures:
        print(f"RULE-003 violation: source files exceed {MAX_NON_TEST_LINES} lines.")
        for path, line_count in failures:
            print(f"{path}: {line_count} lines")
        return 1

    print(f"RULE-003 check passed: all source files are <= {MAX_NON_TEST_LINES} lines.")
    return 0


if __name__ == "__main__":
    sys.exit(main())


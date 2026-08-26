#!/usr/bin/env python3
"""Reject duplicate ADR identifiers before documentation changes reach CI."""

from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import re
import sys

from lint_common import discover_repo_root


ADR_ENTRY = re.compile(r"^\s*- \[ADR-(\d{3})\s+— .+\]\(\./(ADR-\d{3}-[^)]+\.md)\)\s*$")
ADR_FILENAME = re.compile(r"^ADR-(\d{3})-[^.]+\.md$")


def index_entries(index_path: Path) -> list[tuple[int, str, str]]:
    """Return `(line, number, filename)` entries from the ADR index."""
    entries: list[tuple[int, str, str]] = []
    for line_number, line in enumerate(index_path.read_text(encoding="utf-8").splitlines(), 1):
        match = ADR_ENTRY.match(line)
        if match:
            entries.append((line_number, match.group(1), match.group(2)))
    return entries


def find_violations(repo_root: Path) -> list[str]:
    """Report duplicate numbers, malformed entries, and missing ADR targets."""
    index_path = repo_root / "docs/adr/INDEX.md"
    if not index_path.exists():
        return ["docs/adr/INDEX.md: missing ADR index"]

    by_number: dict[str, list[tuple[int, str]]] = defaultdict(list)
    violations: list[str] = []
    for line, number, filename in index_entries(index_path):
        by_number[number].append((line, filename))
        filename_match = ADR_FILENAME.match(filename)
        if filename_match is None:
            violations.append(f"docs/adr/INDEX.md:{line}: malformed ADR filename `{filename}`")
        elif filename_match.group(1) != number:
            violations.append(
                f"docs/adr/INDEX.md:{line}: index ADR-{number} does not match filename `{filename}`"
            )
        elif not (repo_root / "docs/adr" / filename).is_file():
            violations.append(f"docs/adr/INDEX.md:{line}: missing indexed ADR file `{filename}`")

    for number, entries in sorted(by_number.items()):
        if len(entries) > 1:
            lines = ", ".join(str(line) for line, _ in entries)
            violations.append(f"docs/adr/INDEX.md:{lines}: duplicate ADR-{number}")
    return violations


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Reject duplicate or malformed ADR index entries.")
    parser.add_argument("--root", help="Repository root to inspect.")
    args = parser.parse_args(argv[1:])
    violations = find_violations(discover_repo_root(args.root))
    if violations:
        print("ADR index violations:")
        print("\n".join(f"- {violation}" for violation in violations))
        return 1
    print("ADR index: no duplicate ADR identifiers or broken indexed targets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

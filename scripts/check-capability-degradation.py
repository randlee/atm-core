#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import re


@dataclass(frozen=True)
class Violation:
    path: Path
    line_number: int
    label: str
    line: str


FORBIDDEN_PATTERNS = (
    ("replay capability degradation assignment", re.compile(r"\breplay_store\s*=\s*None\b")),
    ("replay capability degradation field", re.compile(r"\breplay_store\s*:\s*None\b")),
)

def iter_rust_sources(repo_root: Path) -> tuple[Path, ...]:
    return tuple(sorted((repo_root / "crates").rglob("*.rs")))


def find_violations(repo_root: Path) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    for path in iter_rust_sources(repo_root):
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            for label, pattern in FORBIDDEN_PATTERNS:
                if pattern.search(line):
                    violations.append(
                        Violation(path.relative_to(repo_root), line_number, label, line.strip())
                    )
    return tuple(violations)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Enforce the Phase X replay-capability no-degradation regression gate on workspace Rust sources."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="Workspace root to scan. Defaults to the repository that owns this script.",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    repo_root = args.repo_root.resolve()
    violations = find_violations(repo_root)
    if violations:
        print("capability-degradation failed")
        for violation in violations:
            print(
                f"{violation.path}:{violation.line_number}: {violation.label}: {violation.line}"
            )
        return 1

    print("capability-degradation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

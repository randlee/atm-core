#!/usr/bin/env python3
"""Find TODO comments in repo files and emit structured rows."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

_ALLOWED_SUFFIXES = {
    ".rs",
}
_INCLUDED_TOP_LEVEL_DIRS = {
    ".just",
    "boundaries",
    "crates",
    "docs",
    "scripts",
}
_IGNORED_DIRS = {
    ".git",
    ".jj",
    ".venv",
    ".worktrees",
    "__pycache__",
    "node_modules",
    "target",
}
_TODO_BODY_RE = re.compile(
    r"TODO(?:\((?P<tag>[^)]+)\))?(?:(?::\s*)|(?:\s+))(?P<text>[^\n\r]*)"
)
_COMMENT_PATTERNS = {
    ".rs": re.compile(r"//(?P<body>.*TODO(?:\(|:|\s).*)$"),
}


@dataclass(frozen=True)
class TodoRow:
    path: str
    line: int
    tag: str
    text: str

    def render(self) -> str:
        return f"{self.path}:{self.line}:{self.tag}:{self.text}"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Recursively find TODO comments in repo source and docs files."
    )
    parser.add_argument(
        "root",
        nargs="?",
        default=".",
        help="repo root to scan (default: current directory)",
    )
    return parser.parse_args(argv)


def iter_candidate_files(root: Path) -> Iterable[Path]:
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if any(part in _IGNORED_DIRS for part in path.parts):
            continue
        rel_path = path.relative_to(root)
        top_level = rel_path.parts[0] if rel_path.parts else ""
        if len(rel_path.parts) > 1 and top_level not in _INCLUDED_TOP_LEVEL_DIRS:
            continue
        if path.suffix not in _ALLOWED_SUFFIXES:
            continue
        yield path


def extract_rows(root: Path) -> list[TodoRow]:
    rows: list[TodoRow] = []
    for path in iter_candidate_files(root):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        rel_path = path.relative_to(root).as_posix()
        line_pattern = _COMMENT_PATTERNS[path.suffix]
        for line_number, line in enumerate(lines, start=1):
            comment_match = line_pattern.search(line)
            if not comment_match:
                continue
            match = _TODO_BODY_RE.search(comment_match.group("body"))
            if not match:
                continue
            tag = (match.group("tag") or "untagged").strip() or "untagged"
            text = match.group("text").strip() or "<empty>"
            rows.append(
                TodoRow(
                    path=rel_path,
                    line=line_number,
                    tag=tag,
                    text=text,
                )
            )
    return rows


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    root = Path(args.root).resolve()
    for row in extract_rows(root):
        print(row.render())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

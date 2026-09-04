#!/usr/bin/env python3
"""Assert that a source file contains a regex pattern, without external tools.

Smoke rows used to shell out to `rg -n <pattern> <file>` for source-pattern
assertions. That made the whole thorough smoke abort with FileNotFoundError on
any host whose child PATH does not resolve ripgrep, before a single row ran.
This module performs the same line-oriented regex search with the Python
standard library and keeps the same exit-code contract:

    0  pattern matched (first match printed as ``path:lineno:line``)
    1  pattern did not match any line
    2  the file could not be read (missing, unreadable, or a directory)
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


EXIT_MATCHED = 0
EXIT_NOT_MATCHED = 1
EXIT_UNREADABLE = 2


class SourcePatternError(RuntimeError):
    """The target file could not be read, so no verdict is possible."""


def read_lines(path: Path) -> list[str]:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as error:
        raise SourcePatternError(f"{path}: cannot read file: {error}") from error
    return text.splitlines()


def search_pattern(path: Path, pattern: str) -> tuple[int, str] | None:
    """Return the first ``(line_number, line_text)`` match, or None.

    Line numbers are 1-based, matching the `rg -n` evidence the smoke rows
    previously emitted.
    """
    try:
        expression = re.compile(pattern)
    except re.error as error:
        raise SourcePatternError(f"invalid pattern {pattern!r}: {error}") from error
    for index, line in enumerate(read_lines(path), start=1):
        if expression.search(line):
            return index, line
    return None


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Exit 0 when a regex pattern matches a line in the target file.",
    )
    parser.add_argument("pattern", help="Python regular expression matched per line.")
    parser.add_argument("path", help="File to search, relative to the working directory.")
    args = parser.parse_args(argv[1:])

    path = Path(args.path)
    try:
        found = search_pattern(path, args.pattern)
    except SourcePatternError as error:
        print(str(error), file=sys.stderr)
        return EXIT_UNREADABLE
    if found is None:
        print(f"{path}: no line matched {args.pattern!r}", file=sys.stderr)
        return EXIT_NOT_MATCHED
    line_number, line = found
    print(f"{path}:{line_number}:{line}")
    return EXIT_MATCHED


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

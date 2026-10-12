#!/usr/bin/env python3
"""Delegate RULE-003 to the published CLI (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
from pathlib import Path
import sys

from sc_lint_binary import exec_lint


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)
    try:
        exec_lint(repo_root, "line-counts", argv[1:])
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
    return 127


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

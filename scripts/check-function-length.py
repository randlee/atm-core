#!/usr/bin/env python3
"""Delegate RULE-002 to the published CLI (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
from pathlib import Path
import sys


JUST_DIR = Path(__file__).resolve().parent.parent / ".just"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from sc_lint_binary import exec_lint


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)
    if "--write-baseline" in argv[1:]:
        python = repo_root / ".sc-lint/venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        os.execv(str(python), [str(python), "-m", "sc_lint.lint_function_length", "--root", str(repo_root), *argv[1:]])
    try:
        exec_lint(repo_root, "function-length", argv[1:])
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
    return 127


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

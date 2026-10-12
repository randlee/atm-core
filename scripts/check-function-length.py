#!/usr/bin/env python3
"""Delegate RULE-002 to the published CLI (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
from pathlib import Path
import sys


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)
    if "--write-baseline" in argv[1:]:
        python = repo_root / ".sc-lint/venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        os.execv(str(python), [str(python), "-m", "sc_lint.lint_function_length", "--root", str(repo_root), *argv[1:]])
    os.execvp(
        "sc-lint",
        ["sc-lint", "lint", "function-length", "--root", str(repo_root), *argv[1:]],
    )
    return 127


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

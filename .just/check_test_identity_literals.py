#!/usr/bin/env python3
"""Delegate identity checks to sc-lint (delete when randlee/sc-lint#347 ships)."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    venv_python = repo_root / ".sc-lint/venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    return subprocess.run(
        [str(venv_python), "-m", "sc_lint.lint_identity_literals", "--root", str(repo_root), *argv[1:]],
        cwd=repo_root,
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

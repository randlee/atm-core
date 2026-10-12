#!/usr/bin/env python3
"""Delegate identity checks to the published CLI (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
from pathlib import Path
import sys


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)
    os.execvp("sc-lint", ["sc-lint", "lint", "identity-literals", "--root", str(repo_root), *argv[1:]])
    return 127


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import sys

from sc_lint_binary import exec_lint


REPO_ROOT = Path(__file__).resolve().parent.parent

def main(argv: list[str]) -> int:
    # sc-lint#373: the managed launcher can resolve a stale Homebrew backend.
    # Delete when randlee/sc-lint#373 ships.
    try:
        return exec_lint(REPO_ROOT, "sc-portability", argv[1:])
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

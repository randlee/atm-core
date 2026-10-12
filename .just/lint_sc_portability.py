#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import sys

from sc_lint_binary import exec_lint


REPO_ROOT = Path(__file__).resolve().parent.parent

# sc-lint#373: the managed launcher can resolve a stale Homebrew backend.
# Delete when randlee/sc-lint#373 ships.
exec_lint(
    REPO_ROOT,
    "sc-portability",
    ["--config", str(REPO_ROOT / "sc-lint.toml"), *sys.argv[1:]],
)

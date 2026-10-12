#!/usr/bin/env python3
from __future__ import annotations

import os
import shutil
import sys


# sc-lint#373: the managed launcher can resolve a stale Homebrew backend.
# Delete when randlee/sc-lint#373 ships.
managed_sc_lint = os.environ.get("SC_LINT_BIN")
search_path = os.environ.get("PATH", "")
if managed_sc_lint:
    managed_sc_lint = os.path.realpath(managed_sc_lint)
    search_path = os.pathsep.join(
        entry
        for entry in search_path.split(os.pathsep)
        if os.path.realpath(os.path.join(entry or os.curdir, "sc-lint")) != managed_sc_lint
    )
sc_lint = shutil.which("sc-lint", path=search_path) or "sc-lint"
os.execvp(
    sc_lint,
    [sc_lint, "lint", "sc-portability", *sys.argv[1:]],
)

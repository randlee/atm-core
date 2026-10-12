#!/usr/bin/env python3
from __future__ import annotations

import os
import sys


# sc-lint#373: the managed launcher can resolve a stale Homebrew backend.
# Delete when randlee/sc-lint#373 ships.
os.execvp(
    "sc-lint",
    ["sc-lint", "lint", "sc-portability", "--config", "sc-lint.toml", *sys.argv[1:]],
)

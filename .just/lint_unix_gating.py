#!/usr/bin/env python3
from __future__ import annotations

import os
import sys


os.execvp(
    "sc-lint",
    ["sc-lint", "lint", "sc-portability", "--config", "sc-lint.toml", *sys.argv[1:]],
)

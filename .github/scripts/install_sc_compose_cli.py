#!/usr/bin/env python3
"""Install the repository-pinned sc-compose CLI for release-preflight tests."""

from __future__ import annotations

import shlex
import subprocess
import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY_ROOT / ".claude"))

from lib.sc_compose_dependency import SC_COMPOSE_INSTALL  # noqa: E402


def main() -> int:
    return subprocess.run(shlex.split(SC_COMPOSE_INSTALL), check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Install a repository-pinned sc-compose CLI for default or parity tests."""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY_ROOT / ".claude"))

from lib.sc_compose_dependency import (  # noqa: E402
    SC_COMPOSE_INSTALL,
    SC_COMPOSE_PARITY_INSTALL,
)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--purpose",
        choices=("default", "parity"),
        default="parity",
        help="select the repository-pinned CLI revision",
    )
    args = parser.parse_args(argv)
    install = SC_COMPOSE_PARITY_INSTALL if args.purpose == "parity" else SC_COMPOSE_INSTALL
    return subprocess.run(shlex.split(install), check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())

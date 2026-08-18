#!/usr/bin/env python3
"""Provision the pinned sc-compose Python bindings used by publish-kit scripts."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


SC_COMPOSE_VERSION = "1.4.1"


def python_path(venv: Path) -> Path:
    """Return the platform-specific interpreter path in a virtual environment."""
    directory = "Scripts" if sys.platform == "win32" else "bin"
    executable = "python.exe" if sys.platform == "win32" else "python"
    return venv / directory / executable


def installed_version(python: Path) -> str | None:
    """Return the wheel version already installed in the managed environment."""
    result = subprocess.run(
        [
            str(python),
            "-c",
            "import sc_compose; from importlib.metadata import version; print(version('sc-compose'))",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--venv", required=True, type=Path, help="managed virtual environment")
    args = parser.parse_args()
    venv = args.venv.resolve()
    python = python_path(venv)
    if not python.is_file():
        subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)

    existing = installed_version(python)
    if existing is None:
        subprocess.run(
            [
                str(python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                f"sc-compose=={SC_COMPOSE_VERSION}",
            ],
            check=True,
            stdout=sys.stderr,
        )
        existing = installed_version(python)
    if existing != SC_COMPOSE_VERSION:
        raise SystemExit(
            "managed environment has incompatible sc-compose wheel "
            f"{existing!r}; expected {SC_COMPOSE_VERSION!r}. Use a new --venv path."
        )
    print(python)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Provision the pinned sc-compose Python bindings used by publish-kit scripts."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


# This is an intentional historical compatibility floor, not a consumer's
# current workspace version. 1.4.1 is the first published wheel with the
# renderer features required by publish-kit templates.
SC_COMPOSE_VERSION = "1.4.1"


def python_path(venv: Path) -> Path:
    """Return the platform-specific interpreter path in a virtual environment."""
    directory = "Scripts" if sys.platform == "win32" else "bin"
    executable = "python.exe" if sys.platform == "win32" else "python"
    return venv / directory / executable


def installed_version(python: Path) -> str | None:
    """Return installed distribution metadata before importing the binding module."""
    result = subprocess.run(
        [
            str(python),
            "-c",
            "from importlib.metadata import version; print(version('sc-compose'))",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def version_components(value: str) -> tuple[int, ...]:
    """Return numeric release components for a stable wheel version."""
    if not re.fullmatch(r"\d+(?:\.\d+)*", value):
        raise SystemExit(
            "cannot verify managed sc-compose wheel version "
            f"{value!r}; required >= {SC_COMPOSE_VERSION}"
        )
    return tuple(int(component) for component in value.split("."))


def require_version_floor(installed: str) -> None:
    """Fail before downstream pytest can import a stale renderer binding."""
    if version_components(installed) < version_components(SC_COMPOSE_VERSION):
        raise SystemExit(
            "managed environment has incompatible sc-compose wheel: stale version "
            f"{installed!r}; required >= {SC_COMPOSE_VERSION}. Use a new --venv path."
        )


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
    if existing is None:
        raise SystemExit(
            "managed environment has incompatible sc-compose wheel: "
            "installation completed but its version could not be determined"
        )
    require_version_floor(existing)
    print(python)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Create a local venv with the same pinned Python tools as GitHub Actions."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys


REPO_ROOT = Path(__file__).resolve().parents[1]
REQUIREMENTS = REPO_ROOT / ".github" / "python-tooling-requirements.txt"
DEFAULT_VENV = REPO_ROOT / ".venv" / "atm-tools"


def venv_python(venv_dir: Path) -> Path:
    if sys.platform == "win32":
        return venv_dir / "Scripts" / "python.exe"
    return venv_dir / "bin" / "python"


def commands(python: str, venv_dir: Path) -> list[list[str]]:
    tool_python = venv_python(venv_dir)
    return [
        [python, "-m", "venv", str(venv_dir)],
        [str(tool_python), "-m", "pip", "install", "--upgrade", "pip"],
        [str(tool_python), "-m", "pip", "install", "--requirement", str(REQUIREMENTS)],
        [str(tool_python), "-m", "pip", "check"],
    ]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python", default=sys.executable, help="Base Python interpreter used to create the venv.")
    parser.add_argument("--venv", type=Path, default=DEFAULT_VENV, help="Destination virtual environment.")
    parser.add_argument("--dry-run", action="store_true", help="Print commands without changing the environment.")
    args = parser.parse_args(argv[1:])

    for command in commands(args.python, args.venv):
        print(" ".join(command))
        if not args.dry_run:
            subprocess.run(command, check=True)

    if not args.dry_run:
        print(f"Installed pinned ATM Python tools in {args.venv}")
        print(f"Use: ATM_PYTHON_CMD={venv_python(args.venv)} just validate")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

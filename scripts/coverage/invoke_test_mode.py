#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import subprocess
import sys


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def run(command: list[str]) -> None:
    subprocess.run(command, cwd=repo_root(), check=True)


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "default"
    if mode == "default":
        run(["cargo", "build", "--workspace"])
        run(["cargo", "test", "--workspace"])
        return 0
    if mode == "coverage":
        run([sys.executable, "scripts/coverage/run.py", "--write-artifacts"])
        return 0
    raise SystemExit(f"unknown test mode: {mode}")


if __name__ == "__main__":
    raise SystemExit(main())

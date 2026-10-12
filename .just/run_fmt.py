#!/usr/bin/env python3
"""Run the published fmt command directly."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    mode = argv[1] if len(argv) > 1 else "check"
    if mode in {"write", "apply"}:
        command = ["cargo", "fmt", "--all"]
    elif mode == "check":
        command = ["cargo", "fmt", "--all", "--check"]
    else:
        print(f"unknown fmt mode: {mode}", file=sys.stderr)
        return 2
    return subprocess.run(command, cwd=repo_root, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

#!/usr/bin/env python3
"""Run the published fmt command directly (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
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
    os.chdir(repo_root)
    return subprocess.run(command, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

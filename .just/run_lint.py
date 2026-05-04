#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


LINT_STEPS = {
    "fmt": "just _lint-fmt",
    "clippy": "just _lint-clippy",
    "version": "just _lint-version",
    "identities": "just _lint-identities",
    "lines": "just _lint-lines",
}


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    target = argv[1] if len(argv) > 1 else "all"

    if target == "all":
        commands = [LINT_STEPS[name] for name in ("fmt", "clippy", "version", "identities", "lines")]
    else:
        command = LINT_STEPS.get(target)
        if command is None:
            valid = ", ".join(["all", *LINT_STEPS.keys()])
            print(f"unknown lint target: {target}", file=sys.stderr)
            print(f"expected one of: {valid}", file=sys.stderr)
            return 2
        commands = [command]

    for command in commands:
        completed = subprocess.run(command, shell=True, cwd=repo_root)
        if completed.returncode != 0:
            return completed.returncode

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

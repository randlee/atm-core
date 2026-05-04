#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys

from lint_common import discover_repo_root


def build_command(repo_root: Path) -> list[str]:
    return [
        "cargo-deny",
        "check",
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run cargo-deny with the repo policy.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    if shutil.which("cargo-deny") is None:
        print("cargo-deny is not installed; install it to run this lint", file=sys.stderr)
        return 2

    completed = subprocess.run(
        build_command(repo_root),
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

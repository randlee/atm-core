#!/usr/bin/env python3
# Delete when randlee/sc-lint#198 ships: the consumer step then runs
# `sc-lint lint sc-boundary` directly instead of through this script.
from __future__ import annotations

from pathlib import Path
import argparse
import os
import subprocess
import sys

from lint_common import discover_repo_root


def command(repo_root: Path) -> list[str]:
    # `.sc-lint/bootstrap` exports SC_LINT_BIN for the resolved product binary.
    binary = os.environ.get("SC_LINT_BIN") or "sc-lint"
    return [binary, "lint", "sc-boundary", "--config", str(repo_root / "sc-lint.toml")]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run the published sc-lint boundary analyzer.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return subprocess.run(command(repo_root), cwd=repo_root, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

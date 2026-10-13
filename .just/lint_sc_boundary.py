#!/usr/bin/env python3
# Delete when randlee/sc-lint#198 ships: the consumer step then runs
# `sc-lint lint sc-boundary` directly instead of through this script.
from __future__ import annotations

from pathlib import Path
import argparse
import sys

from lint_common import discover_repo_root
from sc_lint_binary import exec_lint


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run the published sc-lint boundary analyzer.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    try:
        return exec_lint(repo_root, "sc-boundary", ["--config", str(repo_root / "sc-lint.toml")])
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys

from lint_common import discover_repo_root
from lint_common import workspace_crate_section_lines


def build_command(repo_root: Path) -> list[str]:
    return [
        sys.executable,
        "-c",
        "import sys; from codespell_lib import _script_main; sys.exit(_script_main())",
    ]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run codespell with the repo policy.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    for line in workspace_crate_section_lines(repo_root):
        print(line)

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

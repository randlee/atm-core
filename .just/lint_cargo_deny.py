#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from lint_common import discover_repo_root
from lint_common import workspace_crate_section_lines


DEPRECATED_CONFIG_LINES = (
    "vulnerability = ",
    "unlicensed = ",
)


def build_command(repo_root: Path, config_path: Path) -> list[str]:
    return [
        "cargo-deny",
        "check",
        "--config",
        str(config_path),
        "advisories",
        "bans",
        "licenses",
        "sources",
    ]


def build_runtime_config(repo_root: Path) -> Path:
    source_path = repo_root / "deny.toml"
    text = source_path.read_text(encoding="utf-8")
    filtered_lines = [
        line
        for line in text.splitlines()
        if not any(line.lstrip().startswith(prefix) for prefix in DEPRECATED_CONFIG_LINES)
    ]
    temp_dir = Path(tempfile.mkdtemp(prefix="atm-lint-deny-"))
    runtime_path = temp_dir / "deny.toml"
    runtime_path.write_text("\n".join(filtered_lines).rstrip() + "\n", encoding="utf-8")
    return runtime_path


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run cargo-deny with the repo policy.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    if shutil.which("cargo-deny") is None:
        print("cargo-deny is not installed; install it to run this lint", file=sys.stderr)
        return 2

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    runtime_config = build_runtime_config(repo_root)
    completed = subprocess.run(
        build_command(repo_root, runtime_config),
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

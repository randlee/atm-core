#!/usr/bin/env python3
"""Synchronize repository-neutral publish-kit assets from sc-compose.

The publish kit is shared implementation. Repository-owned release names,
artifacts, destinations, and channel inputs remain in
``release/publish-artifacts.toml`` and are deliberately not copied here.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


AGENT_FILES = (
    ".claude/agents/publisher.md",
    ".claude/agents/publisher-channel-protocol.md",
    ".claude/agents/crates-io-publisher.md",
    ".claude/agents/github-release-publisher.md",
    ".claude/agents/homebrew-publisher.md",
    ".claude/agents/pypi-publisher.md",
    ".claude/agents/scoop-publisher.md",
    ".claude/agents/winget-publisher.md",
    "release/publish-channel-contracts.toml",
    "scripts/release_manifest.py",
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def default_source() -> Path:
    for ancestor in (repo_root(), *repo_root().parents):
        candidate = ancestor / "sc-compose-worktrees" / "review-pr-507"
        if candidate.is_dir():
            return candidate
    return repo_root().parents[2] / "sc-compose-worktrees" / "review-pr-507"


def revision(source: Path) -> str:
    return subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()


def sync(source: Path, destination: Path, *, check: bool) -> int:
    differences: list[str] = []
    for relative in AGENT_FILES:
        source_file = source / relative
        destination_file = destination / relative
        if not source_file.is_file():
            raise ValueError(f"required source file is absent: {source_file}")
        if not destination_file.is_file() or source_file.read_bytes() != destination_file.read_bytes():
            differences.append(relative)

    if not differences:
        print(f"publish-kit prompt assets synchronized from {source} @ {revision(source)}")
        return 0
    if check:
        print("publish-kit prompt assets are not synchronized:", file=sys.stderr)
        print("\n".join(differences), file=sys.stderr)
        return 1

    for relative in differences:
        source_file = source / relative
        destination_file = destination / relative
        destination_file.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_file, destination_file)
    print(f"synchronized {len(differences)} publish-kit prompt asset(s) from {source} @ {revision(source)}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=default_source())
    parser.add_argument("--destination", type=Path, default=repo_root())
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return sync(args.source.resolve(), args.destination.resolve(), check=args.check)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"publish-kit synchronization failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

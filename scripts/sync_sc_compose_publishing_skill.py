#!/usr/bin/env python3
"""Vendor the sc-compose publishing skill into this repository.

The Marketplace package will replace this temporary bridge.  Until then, this
script makes the source checkout and revision explicit, so a skill refresh is
repeatable and reviewable rather than a hand-copied set of files.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


SKILL_RELATIVE_PATH = Path(".claude/skills/publishing")
PROVENANCE_FILE = ".sc-compose-source.json"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def default_source() -> Path:
    """Return the agreed local sc-compose publishing-skill worktree path."""
    for ancestor in (repo_root(), *repo_root().parents):
        candidate = ancestor / "sc-compose-worktrees" / "review-pr-507"
        if candidate.is_dir():
            return candidate
    # Keep the expected sibling location in the error if the worktree is absent.
    return repo_root().parents[2] / "sc-compose-worktrees" / "review-pr-507"


def git_revision(source: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def files_under(directory: Path) -> dict[Path, bytes]:
    return {
        path.relative_to(directory): path.read_bytes()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def provenance(source: Path, revision: str) -> bytes:
    return (
        json.dumps(
            {
                "source_repository": "randlee/sc-compose",
                "source_revision": revision,
                "source_skill_path": str(SKILL_RELATIVE_PATH),
                "sync_script": "scripts/sync_sc_compose_publishing_skill.py",
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    ).encode()


def expected_files(source: Path) -> dict[Path, bytes]:
    source_skill = source / SKILL_RELATIVE_PATH
    if not (source_skill / "SKILL.md").is_file():
        raise ValueError(f"source publishing skill not found: {source_skill}")

    files = files_under(source_skill)
    files[Path(PROVENANCE_FILE)] = provenance(source, git_revision(source))
    return files


def sync(source: Path, destination: Path, *, check: bool) -> int:
    expected = expected_files(source)
    actual = files_under(destination) if destination.exists() else {}
    if actual == expected:
        print(f"publishing skill is synchronized from {source}")
        return 0

    if check:
        print("publishing skill is not synchronized", file=sys.stderr)
        return 1

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary_directory:
        staging = Path(temporary_directory) / "publishing"
        for relative_path, contents in expected.items():
            target = staging / relative_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(contents)

        backup = destination.with_name(f"{destination.name}.previous")
        if backup.exists():
            shutil.rmtree(backup)
        if destination.exists():
            destination.rename(backup)
        staging.rename(destination)
        if backup.exists():
            shutil.rmtree(backup)

    print(f"synchronized publishing skill from {source} to {destination}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=default_source(),
        help="sc-compose worktree root (default: %(default)s)",
    )
    parser.add_argument(
        "--destination",
        type=Path,
        default=repo_root() / SKILL_RELATIVE_PATH,
        help="destination publishing-skill directory (default: %(default)s)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify synchronization without writing",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return sync(args.source.resolve(), args.destination.resolve(), check=args.check)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"publishing skill sync failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Synchronize repository-neutral publish-kit assets from sc-compose.

The publish kit is shared implementation. Repository-owned release names,
artifacts, destinations, and channel inputs remain in
``release/publish-artifacts.toml`` and are deliberately not copied here.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


KIT_FILES = (
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
    "scripts/release_artifacts.py",
    "scripts/ci/validate_publish_order.sh",
    "docs/publishing-agent.md",
    "release/homebrew/formula.rb.j2",
    ".github/workflows/homebrew-publish.yml",
    "release/scoop/manifest.json.j2",
    ".github/workflows/scoop-publish.yml",
    ".github/workflows/winget-publish.yml",
)

# Temporary direct vendoring bridge until the publishing kit is distributed as
# a Marketplace package. Keep this list deliberately explicit and finite.
SKILL_FILES = (
    "SKILL.md",
    "agents/openai.yaml",
    "evals/channel-name-inquiry.md",
    "evals/publisher-preflight.md",
    "evals/publisher-recovery.md",
    "preflight.xml.j2",
    "publish.xml.j2",
    "ref/channel-contracts.md",
    "ref/release-state-strategy.md",
)
SKILL_SOURCE = ".claude/skills/publishing"
SKILL_DESTINATION = ".claude/skills/publishing"
PROVENANCE_FILE = ".sc-compose-source.json"


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


def _source_files(source: Path, include: str) -> dict[Path, bytes]:
    files: dict[Path, bytes] = {}
    if include in {"all", "kit"}:
        files.update({Path(relative): (source / relative).read_bytes() for relative in KIT_FILES})
    if include not in {"all", "skill"}:
        return files
    revision_text = revision(source)
    for relative in SKILL_FILES:
        files[Path(SKILL_DESTINATION) / relative] = (source / SKILL_SOURCE / relative).read_bytes()
    files[Path(SKILL_DESTINATION) / PROVENANCE_FILE] = (
        json.dumps(
            {
                "source_repository": "randlee/sc-compose",
                "source_revision": revision_text,
                "source_skill_path": SKILL_SOURCE,
                "sync_script": "scripts/sync_sc_compose_publish_kit.py",
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    ).encode()
    return files


def sync(source: Path, destination: Path, *, check: bool, include: str) -> int:
    source_files = _source_files(source, include)
    differences: list[str] = []
    for relative, contents in source_files.items():
        destination_file = destination / relative
        if not destination_file.is_file() or destination_file.read_bytes() != contents:
            differences.append(relative.as_posix())

    if not differences:
        print(f"publish-kit assets synchronized from {source} @ {revision(source)}")
        return 0
    if check:
        print("publish-kit assets are not synchronized:", file=sys.stderr)
        print("\n".join(differences), file=sys.stderr)
        return 1

    for relative, contents in source_files.items():
        destination_file = destination / relative
        if relative.as_posix() not in differences:
            continue
        destination_file.parent.mkdir(parents=True, exist_ok=True)
        destination_file.write_bytes(contents)
    print(f"synchronized {len(differences)} publish-kit asset(s) from {source} @ {revision(source)}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=default_source())
    parser.add_argument("--destination", type=Path, default=repo_root())
    parser.add_argument(
        "--only",
        choices=("all", "kit", "skill"),
        default="all",
        help="sync the generic kit, the publisher skill, or both (default: all)",
    )
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return sync(
            args.source.resolve(), args.destination.resolve(), check=args.check, include=args.only
        )
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"publish-kit synchronization failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import json
import subprocess


@dataclass(frozen=True)
class SmokePaths:
    repo_root: Path
    reports_root: Path
    latest_markdown: Path
    timestamped_markdown: Path
    timestamped_json: Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def timestamp_slug(now: datetime | None = None) -> str:
    moment = now or datetime.now(timezone.utc)
    return moment.strftime("%Y-%m-%d-%H-%M-%S")


def level_slug(level: str) -> str:
    normalized = level.strip().lower()
    if normalized not in {"fast", "normal", "thorough"}:
        raise ValueError(f"unsupported smoke level: {level}")
    return "smoke" if normalized == "normal" else f"smoke-{normalized}"


def smoke_paths(level: str, now: datetime | None = None) -> SmokePaths:
    root = repo_root()
    reports_root = root / "reports" / "smoke"
    slug = level_slug(level)
    stamp = timestamp_slug(now)
    latest_name = f"{slug}.md"
    return SmokePaths(
        repo_root=root,
        reports_root=reports_root,
        latest_markdown=reports_root / latest_name,
        timestamped_markdown=reports_root / f"{stamp}-{slug}.md",
        timestamped_json=reports_root / f"{stamp}-{slug}.json",
    )


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def current_binary_sha(root: Path | None = None) -> str:
    working_root = root or repo_root()
    result = subprocess.run(
        ["git", "-C", str(working_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def write_json(path: Path, payload: object) -> None:
    ensure_parent(path)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

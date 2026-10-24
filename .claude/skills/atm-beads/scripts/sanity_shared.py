"""Shared sanity-run validation and phase-ledger path helpers."""
from __future__ import annotations

import math
import subprocess
from pathlib import Path
from typing import TypeGuard


def valid_elapsed_s(value: object) -> TypeGuard[int | float]:
    """Whether elapsed seconds is a finite, non-negative JSON number (excluding bool)."""
    return isinstance(value, (int, float)) and type(value) in (int, float) and math.isfinite(value) and value >= 0


def primary_root(cwd: str | Path | None = None, *, run=subprocess.run) -> Path:
    result = run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        check=True, capture_output=True, text=True, cwd=cwd,
    )
    return Path(result.stdout.strip()).parent


def ledger_path(repo_root: str | Path, phase: str) -> Path:
    return Path(repo_root) / ".sc" / "sanity-log" / f"phase-{phase}.jsonl"

"""One phase's beads for sprint-report, read through the bd-cache.

Never `bd list --all`: that reads every bead in the database, which is slow
and loads the backend on a large one. The cache syncs the phase label once
(incremental after the first run) and serves the rows with pending journal
writes replayed on top.
"""
from __future__ import annotations

import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Protocol

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-bd-orchestration/scripts"))
import bd_cache  # noqa: E402
from operation_models import BeadRecord, is_bead_records  # noqa: E402


class PhaseBeadCache(Protocol):
    def sync(self, phase: str) -> object: ...

    def list(self, phase: str) -> object: ...


def bead_rows(value: object) -> list[BeadRecord]:
    if not is_bead_records(value):
        raise ValueError("bd list returned invalid bead records")
    return value


def direct(repo: Path, phase: str) -> list[BeadRecord]:
    """The phase's beads straight from bd, scoped to its label."""
    command = ["bd", "list", "-l", f"phase-{phase}", "-n", "0", "--status", bd_cache.STATUSES, "--json"]
    result = subprocess.run(command, cwd=repo, capture_output=True, text=True, timeout=bd_cache.BD_TIMEOUT)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(command[:4])} failed: {(result.stderr or result.stdout).strip()[:500]}")
    value: object = json.loads(result.stdout or "[]")
    return bead_rows(value)


def phase_beads(
    repo: Path,
    phase: str,
    cache_factory: Callable[[Path], PhaseBeadCache] | None = None,
) -> list[BeadRecord]:
    """Sync the phase into the cache, then read it; every fallback is announced on stderr."""
    factory = cache_factory if cache_factory is not None else bd_cache.BdCache
    try:
        cache: PhaseBeadCache = factory(repo)
    except (bd_cache.CacheError, OSError) as error:
        print(f"sprint-report: bd-cache unavailable ({error}); reading phase-{phase} from bd", file=sys.stderr)
        return direct(repo, phase)
    try:
        cache.sync(phase)
    except (bd_cache.CacheError, OSError) as error:
        print(f"sprint-report: bd-cache sync failed ({error}); rendering from cached rows", file=sys.stderr)
    rows = bead_rows(cache.list(phase))
    if not rows:
        raise RuntimeError(f"no phase-{phase} beads in the bd-cache or in bd")
    return rows

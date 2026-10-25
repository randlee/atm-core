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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-bd-orchestration/scripts"))
import bd_cache  # noqa: E402


def direct(repo: Path, phase: str) -> list[dict]:
    """The phase's beads straight from bd, scoped to its label."""
    command = ["bd", "list", "-l", f"phase-{phase}", "-n", "0", "--status", bd_cache.STATUSES, "--json"]
    result = subprocess.run(command, cwd=repo, capture_output=True, text=True, timeout=bd_cache.BD_TIMEOUT)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(command[:4])} failed: {(result.stderr or result.stdout).strip()[:500]}")
    return json.loads(result.stdout or "[]") or []


def phase_beads(repo: Path, phase: str, cache_factory=bd_cache.BdCache) -> list[dict]:
    """Sync the phase into the cache, then read it; every fallback is announced on stderr."""
    try:
        cache = cache_factory(repo)
    except (bd_cache.CacheError, OSError) as error:
        print(f"sprint-report: bd-cache unavailable ({error}); reading phase-{phase} from bd", file=sys.stderr)
        return direct(repo, phase)
    try:
        cache.sync(phase)
    except (bd_cache.CacheError, OSError) as error:
        print(f"sprint-report: bd-cache sync failed ({error}); rendering from cached rows", file=sys.stderr)
    rows = cache.list(phase)
    if not rows:
        raise RuntimeError(f"no phase-{phase} beads in the bd-cache or in bd")
    return rows

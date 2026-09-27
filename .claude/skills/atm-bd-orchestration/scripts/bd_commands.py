"""Shared, small wrappers for the `bd` command used by orchestration scripts."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys


_CONTRACT_SCRIPTS = Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"
if str(_CONTRACT_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_CONTRACT_SCRIPTS))

from plan_contract import (  # noqa: E402
    BLOCKS_RELATION,
    DEV_LABEL,
    FINDING_LABEL,
    FIX_LABEL,
    PARENT_CHILD_RELATION,
    SANITY_LABEL,
    priority_for_severity,
)


class BdCommandError(RuntimeError):
    """Raised when `bd` rejects an orchestration graph update."""


def command(argv: list[str], actor: str = "", *, capture: bool = False) -> str:
    """Run a `bd` command, adding the ATM actor without mutating the caller's argv."""
    full_argv = [*argv]
    if actor:
        full_argv.extend(["--actor", actor])
    proc = subprocess.run(full_argv, text=True, capture_output=capture)
    if proc.returncode:
        detail = (proc.stderr or "").strip() or (proc.stdout or "").strip()
        raise BdCommandError(f"{' '.join(full_argv[:3])} failed: {detail}")
    return proc.stdout.strip() if capture else ""

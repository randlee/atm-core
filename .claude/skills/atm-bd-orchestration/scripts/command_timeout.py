"""The one deadline for every bd, git, gh and validate-plan command the gates and the close hook run.

`ATM_BD_GATE_TIMEOUT_S` sets it in seconds; the default is 180. On 2026-10-20 (load 45-55 with five developers
building) a fixed 30s refused three dispatches in a row with `GATE_CANNOT_RUN`.
"""
from __future__ import annotations

import os
from typing import Mapping

TIMEOUT_ENV = "ATM_BD_GATE_TIMEOUT_S"
DEFAULT_COMMAND_TIMEOUT = 180
RAISE_FIX = f"retry once, or raise {TIMEOUT_ENV} if the host is loaded"


def command_timeout(environ: Mapping[str, str] = os.environ) -> int:
    """Seconds one command may take: `ATM_BD_GATE_TIMEOUT_S` when set, else `DEFAULT_COMMAND_TIMEOUT`."""
    raw = environ.get(TIMEOUT_ENV, "")
    if not raw:
        return DEFAULT_COMMAND_TIMEOUT
    try:
        value = int(raw)
    except ValueError:
        value = 0
    if value <= 0:
        raise ValueError(f"{TIMEOUT_ENV}={raw!r} is not a positive integer number of seconds; "
                         f"unset it or set it like {TIMEOUT_ENV}={DEFAULT_COMMAND_TIMEOUT}")
    return value


def import_timeout(environ: Mapping[str, str] = os.environ) -> int:
    """The module-level value: an invalid setting falls back to the default here so import never fails.

    Each script's `main()` calls `command_timeout()` and reports the invalid setting as its own typed error.
    """
    try:
        return command_timeout(environ)
    except ValueError:
        return DEFAULT_COMMAND_TIMEOUT

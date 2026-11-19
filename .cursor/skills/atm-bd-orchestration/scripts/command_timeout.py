# pyright: strict
"""The one deadline for every bd, git, gh and validate-plan command the gates and the close hook run.

`ATM_BD_GATE_TIMEOUT_S` sets it in seconds and wins when set; otherwise the repository key `gate_timeout_s` of
`.claude/project/atm-bd-orchestration.yaml` applies; the default is 180. On 2026-10-20 (load 45-55 with five
developers building) a fixed 30s refused three dispatches in a row with `GATE_CANNOT_RUN`.
"""
from __future__ import annotations

import os
from pathlib import Path
import sys
from typing import Any, Mapping

TIMEOUT_ENV = "ATM_BD_GATE_TIMEOUT_S"
CONFIG_KEY = "gate_timeout_s"
DEFAULT_COMMAND_TIMEOUT = 180
RAISE_FIX = f"retry once, or raise {TIMEOUT_ENV} (or {CONFIG_KEY} in the repository configuration) if the host is loaded"
REPO_ROOT = Path(__file__).resolve().parents[4]


def configured_timeout(root: Path | None = None) -> Any:
    """The repository's `gate_timeout_s`, or None when the repository has no configuration file.

    An unusable configuration (including an invalid `gate_timeout_s`) raises ValueError naming the file.
    """
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
    import repo_config
    try:
        return repo_config.load(REPO_ROOT if root is None else root)[CONFIG_KEY]
    except repo_config.ConfigNotFound:
        return None
    except repo_config.ConfigError as error:
        raise ValueError(str(error)) from error


def command_timeout(environ: Mapping[str, str] = os.environ, configured: Any = None) -> int:
    """Seconds one command may take: `ATM_BD_GATE_TIMEOUT_S` when set, else `configured` (the yaml
    `gate_timeout_s`; None means unset), else `DEFAULT_COMMAND_TIMEOUT`."""
    raw = environ.get(TIMEOUT_ENV, "")
    if not raw:
        if configured is None:
            return DEFAULT_COMMAND_TIMEOUT
        if isinstance(configured, bool) or not isinstance(configured, int) or configured <= 0:
            raise ValueError(f"{CONFIG_KEY}={configured!r} is not a positive integer number of seconds; "
                             f"set it like {CONFIG_KEY}: {DEFAULT_COMMAND_TIMEOUT} in the repository configuration")
        return configured
    try:
        value = int(raw)
    except ValueError:
        value = 0
    if value <= 0:
        raise ValueError(f"{TIMEOUT_ENV}={raw!r} is not a positive integer number of seconds; "
                         f"unset it or set it like {TIMEOUT_ENV}={DEFAULT_COMMAND_TIMEOUT}")
    return value


def repo_timeout(environ: Mapping[str, str] = os.environ, root: Path | None = None) -> int:
    """`command_timeout` with the repository configuration read; the env var, when set, wins without reading it."""
    return command_timeout(environ, None if environ.get(TIMEOUT_ENV) else configured_timeout(root))


def import_timeout(environ: Mapping[str, str] = os.environ) -> int:
    """The module-level value: an invalid setting falls back to the default here so import never fails.

    Each script's `main()` calls `repo_timeout()` and reports the invalid setting as its own typed error.
    """
    try:
        return repo_timeout(environ)
    except ValueError:
        return DEFAULT_COMMAND_TIMEOUT

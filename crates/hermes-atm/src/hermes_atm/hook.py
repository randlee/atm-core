"""Gateway hook entry point installed by :mod:`hermes_atm.installer`."""

from __future__ import annotations

import atexit
import json
from pathlib import Path
from typing import Any, Mapping

from .runtime import HermesAtmRuntime, HermesAtmRuntimeError


_runtime: HermesAtmRuntime | None = None


def _cleanup(runtime: HermesAtmRuntime) -> None:
    """Close only the runtime this hook still owns; safe for repeated exit paths."""

    global _runtime
    if _runtime is runtime:
        runtime.close()
        _runtime = None


def _configuration(path: Path) -> Mapping[str, str]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise HermesAtmRuntimeError(
            f"cannot read Hermes ATM hook configuration: {error}"
        ) from error
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise HermesAtmRuntimeError("unsupported Hermes ATM hook configuration")
    required = ("profile", "atm_home", "identity", "team", "chat_id", "workspace_root")
    if any(
        not isinstance(value.get(name), str) or not value[name].strip()
        for name in required
    ):
        raise HermesAtmRuntimeError("invalid Hermes ATM hook configuration")
    return {name: value[name] for name in required}


async def handle(event_type: str, context: Mapping[str, Any], config_path: Path) -> None:
    """Activate exactly one profile-owned receiver from the public startup seam."""

    global _runtime
    if event_type != "gateway:startup" or _runtime is not None:
        return
    configuration = _configuration(config_path)
    runner = context.get("gateway_runner")
    environment = {
        "ATM_HOME": configuration["atm_home"],
        "ATM_IDENTITY": configuration["identity"],
        "ATM_TEAM": configuration["team"],
        "ATM_CHAT_ID": configuration["chat_id"],
        "ATM_WORKSPACE_ROOT": configuration["workspace_root"],
    }
    runtime = HermesAtmRuntime.from_gateway_runner(
        runner,
        profile=configuration["profile"],
        environment=environment,
    )
    _runtime = runtime
    atexit.register(_cleanup, runtime)

"""Validated paths and identities for feature-smoke evidence."""

from __future__ import annotations

from datetime import datetime, timezone
import os
from pathlib import Path
import platform
import re

try:
    from smoke_common import SmokeError
except ModuleNotFoundError:
    from scripts.smoke.smoke_common import SmokeError


def artifact_segment(value: str, label: str) -> str:
    """Return a stable, path-safe run or host label."""
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", value):
        raise SmokeError(f"{label} must contain only letters, numbers, '.', '_', or '-'")
    return value


def operating_system_label() -> str:
    """Return the stable public OS label used by smoke evidence paths."""
    return {"darwin": "macos"}.get(platform.system().lower(), platform.system().lower())


def smoke_report_directory(root: Path, feature: str) -> tuple[Path, dict[str, str]]:
    """Return an isolated public report directory for one live smoke run."""
    platform_label = artifact_segment(operating_system_label(), "local platform")
    host_label = artifact_segment(platform.node(), "local host name")
    requested_run_id = os.environ.get("ATM_SMOKE_RUN_ID", "").strip()
    run_id = artifact_segment(
        requested_run_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ"),
        "ATM_SMOKE_RUN_ID",
    )
    feature_label = artifact_segment(feature, "smoke feature")
    run_label = f"{run_id}-pid{os.getpid()}-{feature_label}"
    directory = root / "site" / "reports" / "smoke" / platform_label / host_label / run_label
    return directory, {
        "feature": feature_label,
        "host": host_label,
        "platform": platform_label,
        "run_id": run_id,
    }

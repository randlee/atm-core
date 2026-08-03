"""Shared public-evidence redaction policy for report producers."""

from __future__ import annotations

import re
from typing import Any


# Keep this intentionally broad: macOS may expose per-process temporary paths
# below /private/var, while Linux CI commonly uses /home and /tmp roots.
ABSOLUTE_PATH = re.compile(
    r"(?:/Users/[^\s,;]+|/home/[^\s,;]+|/private/tmp/[^\s,;]+|/private/var/[^\s,;]+|/tmp/[^\s,;]+|[A-Za-z]:\\[^\s,;]+)"
)
SENSITIVE_KEYS = frozenset(
    {
        "atm_home",
        "current_dir",
        "daemon_pid",
        "doctor",
        "endpoint",
        "home_dir",
        "path",
        "peer_host",
        "release",
        "worktree_path",
    }
)


def public_string(value: Any, max_length: int = 2000) -> str:
    """Redact absolute paths and bound free-text evidence."""
    return ABSOLUTE_PATH.sub("<redacted-path>", str(value))[:max_length]


def public_value(value: Any) -> Any:
    """Return JSON-compatible evidence with sensitive keys omitted."""
    if isinstance(value, str):
        return public_string(value)
    if isinstance(value, (int, float, bool)) or value is None:
        return value
    if isinstance(value, list):
        return [public_value(item) for item in value]
    if isinstance(value, dict):
        return {
            str(key): public_value(item)
            for key, item in value.items()
            if str(key) not in SENSITIVE_KEYS
        }
    return public_string(value)

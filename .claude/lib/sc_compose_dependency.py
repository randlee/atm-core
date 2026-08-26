"""Single source of truth for the sc-compose dependency contract."""

from __future__ import annotations

import re


SC_COMPOSE_PIN = "1.5.0"
MIN_SC_COMPOSE = (1, 5, 0)
MIN_SC_COMPOSE_TEXT = ">= 1.5.0 (pinned prebuilt release binary)"
MIN_SC_COMPOSE_BINDING = (1, 2, 0)
MIN_SC_COMPOSE_BINDING_TEXT = ">= 1.2.0"
SC_COMPOSE_INSTALL = f"download sc-compose v{SC_COMPOSE_PIN} prebuilt release asset and verify SHA256"
SC_COMPOSE_BINDING_INSTALL = (
    "python3 -m pip install --user --break-system-packages "
    "'sc-compose>=1.2.0'"
)
_VERSION_RE = re.compile(
    r"(?<!\d)(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]+)?"
)


def parse_version(text: str | None) -> tuple[int, int, int] | None:
    """Extract a comparable semantic version from tool output."""

    if not text:
        return None
    match = _VERSION_RE.search(text)
    return tuple(int(part) for part in match.groups()) if match else None

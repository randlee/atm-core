"""Single source of truth for the sc-compose dependency contract."""

from __future__ import annotations

from pathlib import Path
import re
import tomllib


_VERSION_RE = re.compile(
    r"(?<!\d)(\d+)\.(\d+)\.(\d+)(?:[-+][0-9A-Za-z.-]+)?"
)


def _bootstrap_sc_compose_pin() -> str:
    """Read the only CLI and wheel version pin from the bootstrap contract."""

    manifest = Path(__file__).resolve().parents[2] / "tools" / "bootstrap.toml"
    try:
        version = tomllib.loads(manifest.read_text(encoding="utf-8"))["sc-compose"]["version"]
    except (KeyError, OSError, tomllib.TOMLDecodeError) as error:
        raise RuntimeError(f"cannot read sc-compose version from {manifest}") from error
    if not isinstance(version, str) or _VERSION_RE.fullmatch(version) is None:
        raise RuntimeError(f"{manifest} [sc-compose].version must be a release version")
    return version


def _version_tuple(version: str) -> tuple[int, int, int]:
    match = _VERSION_RE.fullmatch(version)
    if match is None:
        raise ValueError(f"not a release version: {version}")
    return tuple(int(part) for part in match.groups())


SC_COMPOSE_PIN = _bootstrap_sc_compose_pin()
MIN_SC_COMPOSE = _version_tuple(SC_COMPOSE_PIN)
MIN_SC_COMPOSE_TEXT = f">= {SC_COMPOSE_PIN} (pinned prebuilt release binary)"
MIN_SC_COMPOSE_BINDING = MIN_SC_COMPOSE
MIN_SC_COMPOSE_BINDING_TEXT = f">= {SC_COMPOSE_PIN}"
SC_COMPOSE_INSTALL = f"download sc-compose v{SC_COMPOSE_PIN} prebuilt release asset and verify SHA256"
SC_COMPOSE_BINDING_INSTALL = (
    "python3 -m pip install --user --break-system-packages "
    f"'sc-compose>={SC_COMPOSE_PIN}'"
)


def parse_version(text: str | None) -> tuple[int, int, int] | None:
    """Extract a comparable semantic version from tool output."""

    if not text:
        return None
    match = _VERSION_RE.search(text)
    return tuple(int(part) for part in match.groups()) if match else None

#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.12"
# dependencies = ["pydantic>=2", "pyyaml", "sc-compose>=1.8.0,<2"]
# ///
"""Repository and phase settings used by atm-bd tools for their own calls."""
from __future__ import annotations

import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
import repo_config


@dataclass(frozen=True)
class PhaseSettings:
    team: str
    lead: str
    beads_dir: Path
    code_checkout: Path | None
    tool_identity: str


class PhaseSettingsError(ValueError):
    """Phase or repository settings needed for an internal atm-bd call are invalid."""


def beads_directory(repo_root: Path, phase: str | None = None) -> Path:
    """Resolve beads storage from the planning repository, with or without a phase."""
    root = Path(repo_root).resolve()
    if phase is not None:
        return phase_settings(root, phase).beads_dir
    return root / ".beads"


def phase_settings(repo_root: Path, phase: str) -> PhaseSettings:
    root = Path(repo_root).resolve()
    phase_path = root / ".atm-bd" / f"phase-{phase}.toml"
    try:
        phase_data = tomllib.loads(phase_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
        raise PhaseSettingsError(f"cannot read {phase_path}: {exc}") from exc

    lead = phase_data.get("lead")
    if not isinstance(lead, str) or not lead.strip():
        raise PhaseSettingsError(f"set lead in {phase_path} to a nonempty ATM identity")
    team = phase_data.get("team")
    if team is None:
        atm_path = root / ".atm.toml"
        try:
            atm_data = tomllib.loads(atm_path.read_text(encoding="utf-8")) if atm_path.exists() else {}
        except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
            raise PhaseSettingsError(f"cannot read atm.default_team from {atm_path}: {exc}") from exc
        atm = atm_data.get("atm") if isinstance(atm_data, dict) else None
        team = atm.get("default_team") if isinstance(atm, dict) else None
    if not isinstance(team, str) or not team.strip():
        raise PhaseSettingsError(
            f"config_invalid: team is unset; add team = \"<ATM team>\" to {phase_path}, "
            f"or set atm.default_team in {root / '.atm.toml'}"
        )

    checkout_value = phase_data.get("code_checkout")
    if checkout_value is not None and (not isinstance(checkout_value, str) or not checkout_value.strip()):
        raise PhaseSettingsError(f"code_checkout in {phase_path} must be a nonempty path")
    checkout = (root / checkout_value).resolve() if checkout_value else None
    try:
        configured_identity = repo_config.load(root).get("tool_identity", "atm-bd@tool")
    except repo_config.ConfigError as exc:
        raise PhaseSettingsError(str(exc)) from exc
    if not isinstance(configured_identity, str) or not configured_identity.strip():
        raise PhaseSettingsError(f"{repo_config.RELATIVE_PATH}: tool_identity must be a nonempty <identity>@<team> address")

    return PhaseSettings(team.strip(), lead.strip(), root / ".beads", checkout, configured_identity.strip())

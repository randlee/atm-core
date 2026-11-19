from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).parents[1]))
import phase_settings as phase_settings_module


def repository(root: Path, *, phase: str, phase_text: str, atm_text: str = '', identity: str | None = None) -> Path:
    (root / ".atm-bd").mkdir(parents=True, exist_ok=True)
    (root / ".atm-bd" / f"phase-{phase}.toml").write_text(phase_text)
    if atm_text:
        (root / ".atm.toml").write_text(atm_text)
    config = root / ".claude/project/atm-bd-orchestration.yaml"
    config.parent.mkdir(parents=True)
    config.write_text(f"tool_identity: {identity}\n" if identity else "max_sanity_rounds: 5\n")
    return root


def test_phase_team_wins_over_repository_default_team(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\nteam = "phase-team"\n',
                      atm_text='[atm]\ndefault_team = "default-team"\n')
    settings = phase_settings_module.phase_settings(root, "x")
    assert (settings.team, settings.lead) == ("phase-team", "phase-lead")
    assert settings.beads_dir == root / ".beads"



def test_phase_team_does_not_read_unneeded_malformed_default(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\nteam = "phase-team"\n',
                      atm_text='[atm\n')
    assert phase_settings_module.phase_settings(root, "x").team == "phase-team"

def test_missing_phase_and_default_team_has_named_fix(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\n')
    try:
        phase_settings_module.phase_settings(root, "x")
    except phase_settings_module.PhaseSettingsError as error:
        assert "config_invalid: team is unset" in str(error)
        assert f"{root}/.atm-bd/phase-x.toml" in str(error)
        assert f"{root}/.atm.toml" in str(error)
    else:
        raise AssertionError("missing team should be refused")


def test_code_checkout_does_not_move_beads_directory(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\nteam = "team"\ncode_checkout = "../checkout"\n')
    settings = phase_settings_module.phase_settings(root, "x")
    checkout = settings.code_checkout
    assert checkout is not None
    assert checkout == (root / "../checkout").resolve()
    assert settings.beads_dir == root / ".beads"


def test_repository_tool_identity_override(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\nteam = "team"\n', identity="custom-bot@tools")
    assert phase_settings_module.phase_settings(root, "x").tool_identity == "custom-bot@tools"


def test_default_tool_identity(tmp_path):
    root = repository(tmp_path, phase="x", phase_text='lead = "phase-lead"\nteam = "team"\n')
    assert phase_settings_module.phase_settings(root, "x").tool_identity == "atm-bd@tool"

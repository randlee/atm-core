"""Unit tests for the canonical sc-compose CLI installer selector."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from unittest.mock import Mock, patch


INSTALLER_PATH = Path(__file__).resolve().parents[1] / "install_sc_compose_cli.py"
SPEC = importlib.util.spec_from_file_location("install_sc_compose_cli", INSTALLER_PATH)
assert SPEC is not None and SPEC.loader is not None
INSTALLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALLER)


def test_default_purpose_uses_the_released_template_cli_revision() -> None:
    completed = Mock(returncode=0)
    with patch.object(INSTALLER.subprocess, "run", return_value=completed) as run:
        assert INSTALLER.main(["--purpose", "default"]) == 0

    assert run.call_args.args[0] == INSTALLER.shlex.split(INSTALLER.SC_COMPOSE_INSTALL)


def test_parity_purpose_uses_the_passthrough_cli_revision() -> None:
    completed = Mock(returncode=0)
    with patch.object(INSTALLER.subprocess, "run", return_value=completed) as run:
        assert INSTALLER.main(["--purpose", "parity"]) == 0

    assert run.call_args.args[0] == INSTALLER.shlex.split(INSTALLER.SC_COMPOSE_PARITY_INSTALL)

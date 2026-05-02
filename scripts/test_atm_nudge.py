"""Unit tests for atm-nudge.py."""
from __future__ import annotations

import importlib.util
import io
import json
import os
import sys
import unittest
from pathlib import Path
from unittest.mock import MagicMock, call, patch

# Load the module from file path (hyphenated name, not importable as-is).
_SCRIPT = Path(__file__).parent / "atm-nudge.py"
_spec = importlib.util.spec_from_file_location("atm_nudge", _SCRIPT)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

PaneLookup = _mod.PaneLookup
ERR_FILE_MISSING = _mod.ERR_FILE_MISSING
ERR_NOT_FOUND = _mod.ERR_NOT_FOUND
ERR_EMPTY_PANE = _mod.ERR_EMPTY_PANE
CODEX_DEFAULT_PANE = _mod.CODEX_DEFAULT_PANE


def _run(args: list[str], *, env: dict[str, str] | None = None) -> tuple[int, dict]:
    """Run main() with mocked subprocess and env; return (exit_code, stderr_json)."""
    stderr_buf = io.StringIO()
    with patch.object(_mod, "nudge_pane") as mock_nudge, \
         patch.dict(os.environ, env or {}, clear=False), \
         patch("sys.stderr", stderr_buf):
        rc = _mod.main(["atm-nudge.py"] + args)
    stderr_val = stderr_buf.getvalue().strip()
    parsed = json.loads(stderr_val) if stderr_val and stderr_val.startswith("{") else {}
    return rc, parsed, mock_nudge


def _run_with_mocked_lookups(
    args: list[str],
    toml: PaneLookup,
    cfg: PaneLookup,
    *,
    team: str = "atm-dev",
) -> tuple[int, dict, MagicMock]:
    stderr_buf = io.StringIO()
    with patch.object(_mod, "read_pane_from_toml", return_value=toml), \
         patch.object(_mod, "read_pane_from_config", return_value=cfg), \
         patch.object(_mod, "resolve_team", return_value=team), \
         patch.object(_mod, "nudge_pane") as mock_nudge, \
         patch.object(_mod, "log"), \
         patch("sys.stderr", stderr_buf):
        rc = _mod.main(["atm-nudge.py"] + args)
    stderr_val = stderr_buf.getvalue().strip()
    parsed = json.loads(stderr_val) if stderr_val and stderr_val.startswith("{") else {}
    return rc, parsed, mock_nudge


class TestNudgePane(unittest.TestCase):
    """nudge_pane validates inputs before touching subprocess."""

    def _call(self, pane_id, recipient, message):
        with patch("subprocess.run"), patch.object(_mod, "log"):
            _mod.nudge_pane(pane_id, recipient, message)

    def test_valid_inputs_accepted(self):
        with patch("subprocess.run") as mock_run, patch.object(_mod, "log"):
            _mod.nudge_pane("%1", "arch-ctm", "<atm/>")
        self.assertEqual(mock_run.call_count, 2)

    def test_empty_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call("", "arch-ctm", "<atm/>")

    def test_whitespace_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call("   ", "arch-ctm", "<atm/>")

    def test_empty_recipient_raises(self):
        with self.assertRaises(ValueError):
            self._call("%1", "", "<atm/>")

    def test_empty_message_raises(self):
        with self.assertRaises(ValueError):
            self._call("%1", "arch-ctm", "")

    def test_non_string_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call(None, "arch-ctm", "<atm/>")

    def test_tmux_calls_order(self):
        with patch("subprocess.run") as mock_run, patch.object(_mod, "log"):
            _mod.nudge_pane("%2", "quality-mgr", "hello")
        calls = mock_run.call_args_list
        self.assertIn("-l", calls[0][0][0])
        self.assertIn("Enter", calls[1][0][0])


class TestCandidateStartDirs(unittest.TestCase):
    """CLAUDE_PROJECT_DIR is first candidate; PWD and cwd follow."""

    def test_claude_project_dir_first(self):
        with patch.dict(os.environ, {"CLAUDE_PROJECT_DIR": "/tmp/proj", "PWD": "/tmp/other"}):
            dirs = _mod.candidate_start_dirs()
        self.assertEqual(dirs[0], Path("/tmp/proj").resolve())

    def test_pwd_used_when_no_claude_project_dir(self):
        env = {k: v for k, v in os.environ.items() if k != "CLAUDE_PROJECT_DIR"}
        env["PWD"] = "/tmp/other"
        with patch.dict(os.environ, env, clear=True):
            dirs = _mod.candidate_start_dirs()
        self.assertIn(Path("/tmp/other").resolve(), dirs)

    def test_deduplication(self):
        with patch.dict(os.environ, {"CLAUDE_PROJECT_DIR": "/tmp/same", "PWD": "/tmp/same"}):
            dirs = _mod.candidate_start_dirs()
        self.assertEqual(dirs.count(Path("/tmp/same").resolve()), 1)


class TestUsage(unittest.TestCase):
    def test_no_args_exits_1(self):
        stderr_buf = io.StringIO()
        with patch("sys.stderr", stderr_buf):
            rc = _mod.main(["atm-nudge.py"])
        self.assertEqual(rc, 1)
        self.assertIn("usage", stderr_buf.getvalue().lower())

    def test_blank_recipient_exits_1(self):
        stderr_buf = io.StringIO()
        with patch("sys.stderr", stderr_buf):
            rc = _mod.main(["atm-nudge.py", "   "])
        self.assertEqual(rc, 1)


class TestOverrideMode(unittest.TestCase):
    """--pane <id> <recipient> <message> bypasses config lookup entirely."""

    def test_override_calls_nudge_directly(self):
        with patch.object(_mod, "nudge_pane") as mock_nudge, \
             patch.object(_mod, "read_pane_from_toml") as mock_toml, \
             patch.object(_mod, "read_pane_from_config") as mock_cfg, \
             patch.object(_mod, "resolve_team", return_value="atm-dev"):
            rc = _mod.main(["atm-nudge.py", "--pane", "%1", "arch-ctm", "<atm/>"])
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", "arch-ctm", "<atm/>")
        mock_toml.assert_not_called()
        mock_cfg.assert_not_called()

    def test_override_without_message_builds_default(self):
        with patch.object(_mod, "nudge_pane") as mock_nudge, \
             patch.object(_mod, "resolve_team", return_value="atm-dev"), \
             patch.object(_mod, "read_pane_from_toml"), \
             patch.object(_mod, "read_pane_from_config"):
            rc = _mod.main(["atm-nudge.py", "--pane", "%1", "arch-ctm"])
        self.assertEqual(rc, 0)
        _, recipient, message = mock_nudge.call_args[0]
        self.assertEqual(recipient, "arch-ctm")
        self.assertIn("read atm --team atm-dev", message)


class TestHappyPath(unittest.TestCase):
    def test_matching_panes_nudges_and_exits_0(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%1", None, None)
        rc, parsed, mock_nudge = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once()
        self.assertEqual(parsed, {})  # no JSON output on success


class TestPaneMismatch(unittest.TestCase):
    def test_error_code(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        rc, parsed, mock_nudge = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "pane_mismatch")

    def test_nudge_not_called(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        rc, parsed, mock_nudge = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        mock_nudge.assert_not_called()

    def test_nudge_command_uses_toml_pane(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        self.assertIn("--pane %1", parsed["nudge_command"])

    def test_call_to_action_present_and_nonempty(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        self.assertIsInstance(parsed["call_to_action"], list)
        self.assertTrue(all(s.strip() for s in parsed["call_to_action"]))

    def test_call_to_action_says_stop(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        self.assertTrue(any("STOP" in line for line in parsed["call_to_action"]))

    def test_fix_mentions_both_configs(self):
        toml = PaneLookup("%1", None, None)
        cfg = PaneLookup("%9", None, None)
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], toml, cfg)
        fix_text = " ".join(parsed["fix"])
        self.assertIn(".atm.toml", fix_text)
        self.assertIn("config.json", fix_text)


class TestConfigFileMissing(unittest.TestCase):
    """TOML pane known; config.json file does not exist."""

    def setUp(self):
        self.toml = PaneLookup("%1", None, None)
        self.cfg = PaneLookup(None, ERR_FILE_MISSING, "config.json not found for team 'atm-dev'")

    def test_error_code(self):
        rc, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "config_file_missing")

    def test_nudge_command_uses_toml_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertIn("--pane %1", parsed["nudge_command"])

    def test_call_to_action_mentions_missing_file(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        cta = " ".join(parsed["call_to_action"])
        self.assertIn("missing", cta.lower())

    def test_fix_says_create(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertTrue(any("Create" in f or "create" in f for f in parsed["fix"]))


class TestRecipientNotInConfig(unittest.TestCase):
    """TOML pane known; recipient missing from existing config.json."""

    def setUp(self):
        self.toml = PaneLookup("%1", None, None)
        self.cfg = PaneLookup(None, ERR_NOT_FOUND, "'arch-ctm' not in team 'atm-dev' members")

    def test_error_code(self):
        rc, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "recipient_not_in_config")

    def test_nudge_command_uses_toml_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertIn("--pane %1", parsed["nudge_command"])

    def test_fix_says_add(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertTrue(any("Add" in f for f in parsed["fix"]))


class TestTomlFileMissing(unittest.TestCase):
    """.atm.toml not found; config.json pane known."""

    def setUp(self):
        self.toml = PaneLookup(None, ERR_FILE_MISSING, ".atm.toml not found in any parent directory")
        self.cfg = PaneLookup("%2", None, None)

    def test_error_code(self):
        rc, parsed, _ = _run_with_mocked_lookups(["quality-mgr"], self.toml, self.cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "toml_file_missing")

    def test_nudge_command_uses_config_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["quality-mgr"], self.toml, self.cfg)
        self.assertIn("--pane %2", parsed["nudge_command"])

    def test_call_to_action_mentions_missing_file(self):
        _, parsed, _ = _run_with_mocked_lookups(["quality-mgr"], self.toml, self.cfg)
        cta = " ".join(parsed["call_to_action"])
        self.assertIn("missing", cta.lower())


class TestRecipientNotInToml(unittest.TestCase):
    """Config pane known; recipient missing from existing .atm.toml."""

    def setUp(self):
        self.toml = PaneLookup(None, ERR_NOT_FOUND, "'quality-mgr' not found in .atm.toml")
        self.cfg = PaneLookup("%2", None, None)

    def test_error_code(self):
        rc, parsed, _ = _run_with_mocked_lookups(["quality-mgr"], self.toml, self.cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "recipient_not_in_toml")

    def test_nudge_command_uses_config_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["quality-mgr"], self.toml, self.cfg)
        self.assertIn("--pane %2", parsed["nudge_command"])


class TestNeitherFound(unittest.TestCase):
    """Neither source has a pane for recipient; falls back to CODEX_DEFAULT_PANE."""

    def setUp(self):
        self.toml = PaneLookup(None, ERR_NOT_FOUND, "'arch-ctm' not found")
        self.cfg = PaneLookup(None, ERR_NOT_FOUND, "'arch-ctm' not in team 'atm-dev' members")

    def test_error_code(self):
        rc, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertEqual(rc, 1)
        self.assertEqual(parsed["error_code"], "pane_not_configured")

    def test_nudge_command_uses_codex_default_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        self.assertIn(f"--pane {CODEX_DEFAULT_PANE}", parsed["nudge_command"])

    def test_call_to_action_mentions_default_pane(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        cta = " ".join(parsed["call_to_action"])
        self.assertIn(CODEX_DEFAULT_PANE, cta)

    def test_fix_mentions_both_configs(self):
        _, parsed, _ = _run_with_mocked_lookups(["arch-ctm"], self.toml, self.cfg)
        fix_text = " ".join(parsed["fix"])
        self.assertIn(".atm.toml", fix_text)
        self.assertIn("config.json", fix_text)


class TestJsonStructure(unittest.TestCase):
    """All error paths emit valid JSON with required fields."""

    REQUIRED_FIELDS = {"status", "error_code", "recipient", "team", "detail",
                       "call_to_action", "nudge_command", "fix"}

    def _assert_required_fields(self, parsed: dict) -> None:
        for field in self.REQUIRED_FIELDS:
            self.assertIn(field, parsed, f"Missing field: {field}")
        self.assertEqual(parsed["status"], "error")
        self.assertIsInstance(parsed["call_to_action"], list)
        self.assertGreater(len(parsed["call_to_action"]), 0)
        self.assertIsInstance(parsed["fix"], list)
        self.assertGreater(len(parsed["fix"]), 0)
        self.assertIn("--pane", parsed["nudge_command"])
        self.assertIn("atm-nudge.py", parsed["nudge_command"])

    def test_mismatch_has_required_fields(self):
        _, parsed, _ = _run_with_mocked_lookups(
            ["arch-ctm"],
            PaneLookup("%1", None, None),
            PaneLookup("%9", None, None),
        )
        self._assert_required_fields(parsed)

    def test_config_file_missing_has_required_fields(self):
        _, parsed, _ = _run_with_mocked_lookups(
            ["arch-ctm"],
            PaneLookup("%1", None, None),
            PaneLookup(None, ERR_FILE_MISSING, "not found"),
        )
        self._assert_required_fields(parsed)

    def test_toml_file_missing_has_required_fields(self):
        _, parsed, _ = _run_with_mocked_lookups(
            ["arch-ctm"],
            PaneLookup(None, ERR_FILE_MISSING, "not found"),
            PaneLookup("%1", None, None),
        )
        self._assert_required_fields(parsed)

    def test_neither_found_has_required_fields(self):
        _, parsed, _ = _run_with_mocked_lookups(
            ["arch-ctm"],
            PaneLookup(None, ERR_NOT_FOUND, "not found"),
            PaneLookup(None, ERR_NOT_FOUND, "not found"),
        )
        self._assert_required_fields(parsed)

    def test_nudge_command_contains_full_xml_message(self):
        _, parsed, _ = _run_with_mocked_lookups(
            ["arch-ctm"],
            PaneLookup(None, ERR_NOT_FOUND, "not found"),
            PaneLookup(None, ERR_NOT_FOUND, "not found"),
        )
        self.assertIn("<atm>", parsed["nudge_command"])
        self.assertIn("read atm --team", parsed["nudge_command"])


if __name__ == "__main__":
    unittest.main()

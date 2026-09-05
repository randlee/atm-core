"""Unit tests for atm-nudge.py."""
from __future__ import annotations

import importlib.util
import io
import json
import os
import shlex
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Load the module from file path (hyphenated name, not importable as-is).
_SCRIPT = Path(__file__).parent / "atm-nudge.py"
_SPEC = importlib.util.spec_from_file_location("atm_nudge", _SCRIPT)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)

PaneLookup = _MOD.PaneLookup
ERR_EMPTY_PANE = _MOD.ERR_EMPTY_PANE
ERR_INVALID_STRUCTURE = _MOD.ERR_INVALID_STRUCTURE
ERR_NOT_FOUND = _MOD.ERR_NOT_FOUND
ERR_PARSE_ERROR = _MOD.ERR_PARSE_ERROR
ERR_COMMAND_FAILED = _MOD.ERR_COMMAND_FAILED
CODEX_DEFAULT_PANE = _MOD.CODEX_DEFAULT_PANE
TEST_TEAM = "test-team"
TEST_AGENT = "test-agent"
TEST_TEAM_LEAD = "test-lead"
TEST_QM = "test-qm"
TEST_TEAM_LEAD_ADDR = f"{TEST_TEAM_LEAD}@{TEST_TEAM}"


def _parse_json(text: str) -> dict:
    stripped = text.strip()
    if not stripped.startswith("{"):
        return {}
    return json.loads(stripped)


def _run_with_mocked_lookups(
    args: list[str],
    roster: PaneLookup,
    *,
    team: str = TEST_TEAM,
) -> tuple[int, dict, dict, MagicMock]:
    stderr_buf = io.StringIO()
    stdout_buf = io.StringIO()
    with (
        patch.object(_MOD, "read_pane_from_roster", return_value=roster),
        patch.object(_MOD, "resolve_team", return_value=team),
        patch.object(_MOD, "read_post_send_payload", return_value={}),
        patch.object(_MOD, "nudge_pane") as mock_nudge,
        patch.object(_MOD, "log"),
        patch("sys.stderr", stderr_buf),
        patch("sys.stdout", stdout_buf),
    ):
        rc = _MOD.main(["atm-nudge.py"] + args)
    return rc, _parse_json(stderr_buf.getvalue()), _parse_json(stdout_buf.getvalue()), mock_nudge


class TestNudgePane(unittest.TestCase):
    """nudge_pane validates inputs before touching subprocess."""

    def _call(self, pane_id, recipient, message):
        with patch("subprocess.run"), patch.object(_MOD, "log"):
            _MOD.nudge_pane(pane_id, recipient, message)

    def test_valid_inputs_accepted(self):
        with patch("subprocess.run") as mock_run, patch.object(_MOD, "log"):
            _MOD.nudge_pane("%1", TEST_AGENT, "<atm/>")
        self.assertEqual(mock_run.call_count, 2)

    def test_empty_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call("", TEST_AGENT, "<atm/>")

    def test_whitespace_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call("   ", TEST_AGENT, "<atm/>")

    def test_empty_recipient_raises(self):
        with self.assertRaises(ValueError):
            self._call("%1", "", "<atm/>")

    def test_empty_message_raises(self):
        with self.assertRaises(ValueError):
            self._call("%1", TEST_AGENT, "")

    def test_non_string_pane_raises(self):
        with self.assertRaises(ValueError):
            self._call(None, TEST_AGENT, "<atm/>")

    def test_tmux_calls_order(self):
        with patch("subprocess.run") as mock_run, patch.object(_MOD, "log"):
            _MOD.nudge_pane("%2", TEST_QM, "hello")
        calls = mock_run.call_args_list
        self.assertIn("-l", calls[0][0][0])
        self.assertIn("Enter", calls[1][0][0])


class TestBuildNudgeCommand(unittest.TestCase):
    def test_build_nudge_command_round_trips_with_single_quote_message(self):
        message = "<atm><action>it's urgent</action></atm>"
        command = _MOD.build_nudge_command("%7", TEST_QM, message)
        argv = shlex.split(command)
        self.assertEqual(
            argv,
            [
                _MOD.sys.executable or "python3",
                str(_SCRIPT.resolve()),
                "--pane",
                "%7",
                TEST_QM,
                message,
            ],
        )


class TestCandidateStartDirs(unittest.TestCase):
    def test_claude_project_dir_first(self):
        with patch.dict(
            os.environ,
            {
                "CLAUDE_PROJECT_DIR": "/tmp/proj",
                "PWD": "/tmp/other",
                "HOME": "/tmp/home",
                "USERPROFILE": "/tmp/home",
            },
        ):
            with patch("os.getcwd", return_value="/tmp/cwd"):
                dirs = _MOD.candidate_start_dirs()
        self.assertEqual(dirs[0], Path("/tmp/proj").resolve())

    def test_pwd_used_when_no_claude_project_dir(self):
        env = {k: v for k, v in os.environ.items() if k != "CLAUDE_PROJECT_DIR"}
        env["PWD"] = "/tmp/other"
        env["HOME"] = "/tmp/home"
        env["USERPROFILE"] = "/tmp/home"
        with patch.dict(os.environ, env, clear=True):
            with patch("os.getcwd", return_value="/tmp/cwd"):
                dirs = _MOD.candidate_start_dirs()
        self.assertIn(Path("/tmp/other").resolve(), dirs)

    def test_deduplication(self):
        with patch.dict(
            os.environ,
            {
                "CLAUDE_PROJECT_DIR": "/tmp/same",
                "PWD": "/tmp/same",
                "HOME": "/tmp/home",
                "USERPROFILE": "/tmp/home",
            },
        ):
            with patch("os.getcwd", return_value="/tmp/same"):
                dirs = _MOD.candidate_start_dirs()
        self.assertEqual(dirs.count(Path("/tmp/same").resolve()), 1)

    def test_ignores_getcwd_failure(self):
        with patch.dict(
            os.environ,
            {
                "CLAUDE_PROJECT_DIR": "/tmp/proj",
                "HOME": "/tmp/home",
                "USERPROFILE": "/tmp/home",
            },
            clear=True,
        ):
            with patch("os.getcwd", side_effect=OSError("gone")):
                dirs = _MOD.candidate_start_dirs()
        self.assertEqual(dirs, [Path("/tmp/proj").resolve()])


class TestReadPaneFromRoster(unittest.TestCase):
    def test_reports_invalid_members_structure(self):
        process = MagicMock(returncode=0, stdout='{"members": {}}', stderr="")
        with patch("subprocess.run", return_value=process):
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {})
        self.assertEqual(result.error_code, ERR_INVALID_STRUCTURE)

    def test_reports_missing_member(self):
        process = MagicMock(returncode=0, stdout='{"team":"test-team","members":[]}', stderr="")
        with patch("subprocess.run", return_value=process):
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {})
        self.assertEqual(result.error_code, ERR_NOT_FOUND)

    def test_reads_tmux_pane_id_from_members_json(self):
        process = MagicMock(
            returncode=0,
            stdout=json.dumps(
                {
                    "team": TEST_TEAM,
                    "members": [
                        {"name": TEST_AGENT, "tmux_pane_id": "%17"},
                    ],
                }
            ),
            stderr="",
        )
        with patch("subprocess.run", return_value=process):
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {"sender": TEST_TEAM_LEAD})
        self.assertEqual(result.pane_id, "%17")
        self.assertEqual(result.source_path, "atm members --team <team> --json")

    def test_roster_match_skips_toml_fallback_lookup(self):
        process = MagicMock(
            returncode=0,
            stdout=json.dumps(
                {
                    "team": TEST_TEAM,
                    "members": [
                        {"name": TEST_AGENT, "tmux_pane_id": "%17"},
                    ],
                }
            ),
            stderr="",
        )
        with (
            patch.dict(os.environ, {}, clear=True),
            patch("subprocess.run", return_value=process) as mock_run,
            patch.object(
                _MOD,
                "discover_atm_toml",
                side_effect=AssertionError("pane lookup must not consult .atm.toml"),
            ),
        ):
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {"sender": TEST_TEAM_LEAD})
        self.assertEqual(result.pane_id, "%17")
        self.assertEqual(
            mock_run.call_args.args[0],
            ["atm", "members", "--team", TEST_TEAM, "--json"],
        )
        self.assertEqual(mock_run.call_args.kwargs["env"]["ATM_TEAM"], TEST_TEAM)
        self.assertEqual(mock_run.call_args.kwargs["env"]["ATM_IDENTITY"], TEST_TEAM_LEAD)

    def test_reports_command_failure(self):
        process = MagicMock(returncode=1, stdout="", stderr="boom")
        with patch("subprocess.run", return_value=process):
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {})
        self.assertEqual(result.error_code, ERR_COMMAND_FAILED)
        self.assertIn("boom", result.error_msg)


class TestUsage(unittest.TestCase):
    def test_no_args_exits_1(self):
        stderr_buf = io.StringIO()
        with patch("sys.stderr", stderr_buf):
            rc = _MOD.main(["atm-nudge.py"])
        self.assertEqual(rc, 1)
        self.assertIn("usage", stderr_buf.getvalue().lower())

    def test_blank_recipient_exits_1(self):
        stderr_buf = io.StringIO()
        with patch("sys.stderr", stderr_buf):
            rc = _MOD.main(["atm-nudge.py", "   "])
        self.assertEqual(rc, 1)


class TestOverrideMode(unittest.TestCase):
    def test_override_calls_nudge_directly(self):
        with (
            patch.object(_MOD, "nudge_pane") as mock_nudge,
            patch.object(_MOD, "read_pane_from_roster") as mock_roster,
            patch.object(_MOD, "resolve_team", return_value=TEST_TEAM),
        ):
            rc = _MOD.main(["atm-nudge.py", "--pane", "%1", TEST_AGENT, "<atm/>"])
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", TEST_AGENT, "<atm/>")
        mock_roster.assert_not_called()

    def test_override_without_message_builds_default(self):
        with (
            patch.object(_MOD, "nudge_pane") as mock_nudge,
            patch.object(_MOD, "resolve_team", return_value=TEST_TEAM),
            patch.object(_MOD, "read_post_send_payload", return_value={}),
            patch.object(_MOD, "read_pane_from_roster"),
        ):
            rc = _MOD.main(["atm-nudge.py", "--pane", "%1", TEST_AGENT])
        self.assertEqual(rc, 0)
        _, recipient, message = mock_nudge.call_args[0]
        self.assertEqual(recipient, TEST_AGENT)
        self.assertIn("atm read", message)


class TestBuildMessage(unittest.TestCase):
    def test_default_send_message_requests_assigned_task_execution(self):
        message = _MOD.build_message(TEST_TEAM, {})
        self.assertIn("atm read", message)
        self.assertIn("execute the assigned task", message)
        self.assertIn('busy="after-current-task"', message)
        self.assertIn("<description></description>", message)

    def test_default_send_message_targets_message_when_id_is_present(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {"message_id": "01JSENDTEST0000000000000000"},
        )
        self.assertIn(
            "atm read --message-id 01JSENDTEST0000000000000000",
            message,
        )

    def test_send_message_includes_message_id_as_attribute_when_present(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {"message_id": "01JSENDTEST0000000000000000"},
        )
        self.assertIn('message-id="01JSENDTEST0000000000000000"', message)
        self.assertIn("execute the assigned task", message)

    def test_send_message_includes_description_when_present(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {
                "message_id": "01JSENDTEST0000000000000000",
                "summary": "review failing smoke lane",
            },
        )
        self.assertIn('message-id="01JSENDTEST0000000000000000"', message)
        self.assertIn("<description>review failing smoke lane</description>", message)

    def test_requires_ack_message_includes_ack_action(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {"requires_ack": True, "message_id": "01JREQACK00000000000000000"},
        )
        self.assertIn("<action>ack the message</action>", message)
        self.assertIn('message-id="01JREQACK00000000000000000"', message)
        self.assertIn("execute the assigned task", message)

    def test_task_message_uses_task_element(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {
                "message_id": "01JTASKTEST0000000000000000",
                "task_id": "AD.22",
                "description": "finish cleanup",
            },
        )
        self.assertIn('<task id="AD.22">finish cleanup</task>', message)
        self.assertNotIn("<description>", message)

    def test_ack_message_uses_compact_ack_shape(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {
                "is_ack": True,
                "from": TEST_TEAM_LEAD_ADDR,
                "message_id": "01JACKTEST00000000000000000",
            },
        )
        self.assertEqual(
            message,
            f'<atm from="{TEST_TEAM_LEAD_ADDR}" message-id="01JACKTEST00000000000000000" kind="ack"/>',
        )

    def test_ack_task_message_uses_compact_ack_shape_with_task_id(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {
                "is_ack": True,
                "from": TEST_TEAM_LEAD_ADDR,
                "message_id": "01JACKTASK0000000000000000",
                "task_id": "AD.22",
            },
        )
        self.assertEqual(
            message,
            f'<atm from="{TEST_TEAM_LEAD_ADDR}" message-id="01JACKTASK0000000000000000" kind="ack" task-id="AD.22"/>',
        )


class TestMainBehavior(unittest.TestCase):
    def test_roster_match_nudges_without_warning(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup("%1", None, None, "atm members --team <team> --json"),
        )
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", TEST_AGENT, unittest.mock.ANY)
        self.assertEqual(stderr_json, {})
        self.assertEqual(stdout_json, {})

    def test_roster_match_uses_roster_pane(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup("%5", None, None, "atm members --team <team> --json"),
        )
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%5", TEST_AGENT, unittest.mock.ANY)
        self.assertEqual(stderr_json, {})
        self.assertEqual(stdout_json, {})

    def test_roster_failure_emits_manual_nudge_and_fix_call_to_action(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_QM],
            PaneLookup(None, ERR_EMPTY_PANE, "bad roster", "atm members --team <team> --json"),
        )
        self.assertEqual(rc, 1)
        mock_nudge.assert_not_called()
        self.assertEqual(stdout_json, {})
        self.assertEqual(stderr_json["status"], "error")
        self.assertIn("Run nudge_command NOW", " ".join(stderr_json["call_to_action"]))
        self.assertIn("VERIFY the pane id", " ".join(stderr_json["call_to_action"]))
        self.assertIn(f"--pane {CODEX_DEFAULT_PANE}", stderr_json["nudge_command"])
        self.assertIn("Repair canonical ATM roster pane metadata", " ".join(stderr_json["fix"]))

    def test_missing_roster_member_uses_default_manual_pane_hint(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup(None, ERR_NOT_FOUND, "missing member", "atm members --team <team> --json"),
        )
        self.assertEqual(rc, 1)
        mock_nudge.assert_not_called()
        self.assertEqual(stdout_json, {})
        self.assertIn(f"--pane {CODEX_DEFAULT_PANE}", stderr_json["nudge_command"])
        self.assertIn("VERIFY the pane id", " ".join(stderr_json["call_to_action"]))

    def test_error_payload_includes_input_and_resolution_context(self):
        rc, stderr_json, _, _ = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup(None, ERR_COMMAND_FAILED, "missing", "atm members --team <team> --json"),
        )
        self.assertEqual(rc, 1)
        self.assertIn("input", stderr_json)
        self.assertIn("pane_resolution", stderr_json)
        self.assertEqual(stderr_json["input"]["recipient"], TEST_AGENT)
        self.assertEqual(stderr_json["pane_resolution"]["authoritative_source"], "atm roster")


if __name__ == "__main__":
    unittest.main()

"""Unit tests for atm-nudge.py."""
from __future__ import annotations

import importlib.util
import io
import json
import os
import shlex
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Load the module from file path (hyphenated name, not importable as-is).
_SCRIPT = Path(__file__).parent / "atm-nudge.py"
_SPEC = importlib.util.spec_from_file_location("atm_nudge", _SCRIPT)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)

PaneLookup = _MOD.PaneLookup
ERR_AMBIGUOUS = _MOD.ERR_AMBIGUOUS
ERR_EMPTY_PANE = _MOD.ERR_EMPTY_PANE
ERR_FILE_MISSING = _MOD.ERR_FILE_MISSING
ERR_INVALID_STRUCTURE = _MOD.ERR_INVALID_STRUCTURE
ERR_NOT_FOUND = _MOD.ERR_NOT_FOUND
ERR_PARSE_ERROR = _MOD.ERR_PARSE_ERROR
ERR_COMMAND_FAILED = _MOD.ERR_COMMAND_FAILED
CODEX_DEFAULT_PANE = _MOD.CODEX_DEFAULT_PANE
TEST_TEAM = "test-team"
TEST_AGENT = "test-agent"
TEST_TEAM_LEAD = "test-lead"
TEST_QM = "test-qm"


def _parse_json(text: str) -> dict:
    stripped = text.strip()
    if not stripped.startswith("{"):
        return {}
    return json.loads(stripped)


def _run_with_mocked_lookups(
    args: list[str],
    roster: PaneLookup,
    toml: PaneLookup,
    *,
    team: str = TEST_TEAM,
) -> tuple[int, dict, dict, MagicMock]:
    stderr_buf = io.StringIO()
    stdout_buf = io.StringIO()
    with (
        patch.object(_MOD, "read_pane_from_roster", return_value=roster),
        patch.object(_MOD, "read_pane_from_toml", return_value=toml),
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


class TestReadPaneFromToml(unittest.TestCase):
    def _with_project(self, toml_text: str, fn):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            project = root / "repo" / "nested"
            project.mkdir(parents=True)
            (root / "repo" / ".atm.toml").write_text(toml_text, encoding="utf-8")
            with patch.dict(
                os.environ,
                {
                    "CLAUDE_PROJECT_DIR": str(project),
                    "PWD": str(project),
                    "HOME": str(root / "home"),
                    "USERPROFILE": str(root / "home"),
                },
                clear=False,
            ):
                with patch("os.getcwd", return_value=str(project)):
                    fn(root / "repo" / ".atm.toml")

    def test_reads_team_specific_match(self):
        def run(path: Path):
            result = _MOD.read_pane_from_toml(TEST_QM, TEST_TEAM)
            self.assertEqual(result.pane_id, "%2")
            self.assertEqual(Path(result.source_path), path.resolve())

        self._with_project(
            f"""
[atm]
default_team = "{TEST_TEAM}"

[rmux]

[[rmux.windows]]
name = "agents"
[[rmux.windows.panes]]
name = "{TEST_QM}"
tmux_pane_id = "%2"
env = {{ ATM_TEAM = "{TEST_TEAM}" }}
[[rmux.windows.panes]]
name = "{TEST_QM}"
tmux_pane_id = "%9"
env = {{ ATM_TEAM = "schook" }}
""",
            run,
        )

    def test_falls_back_to_single_unscoped_match(self):
        def run(_path: Path):
            result = _MOD.read_pane_from_toml(TEST_AGENT, TEST_TEAM)
            self.assertEqual(result.pane_id, "%1")

        self._with_project(
            f"""
[atm]
default_team = "{TEST_TEAM}"

[rmux]

[[rmux.windows]]
name = "agents"
[[rmux.windows.panes]]
name = "{TEST_AGENT}"
tmux_pane_id = "%1"
""",
            run,
        )

    def test_reports_ambiguous_same_team_match(self):
        def run(_path: Path):
            result = _MOD.read_pane_from_toml(TEST_QM, TEST_TEAM)
            self.assertEqual(result.error_code, ERR_AMBIGUOUS)
            self.assertIn("%2", result.error_msg)
            self.assertIn("%7", result.error_msg)

        self._with_project(
            f"""
[atm]
default_team = "{TEST_TEAM}"

[rmux]

[[rmux.windows]]
name = "agents"
[[rmux.windows.panes]]
name = "{TEST_QM}"
tmux_pane_id = "%2"
env = {{ ATM_TEAM = "{TEST_TEAM}" }}
[[rmux.windows.panes]]
name = "{TEST_QM}"
tmux_pane_id = "%7"
env = {{ ATM_TEAM = "{TEST_TEAM}" }}
""",
            run,
        )

    def test_reports_parse_error(self):
        def run(path: Path):
            result = _MOD.read_pane_from_toml(TEST_AGENT, TEST_TEAM)
            self.assertEqual(result.error_code, ERR_PARSE_ERROR)
            self.assertIn(str(path), result.error_msg)

        self._with_project("not valid toml =", run)

    def test_reports_file_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with patch.dict(
                os.environ,
                {
                    "CLAUDE_PROJECT_DIR": str(root),
                    "PWD": str(root),
                    "HOME": str(root / "home"),
                    "USERPROFILE": str(root / "home"),
                },
                clear=False,
            ):
                with patch("os.getcwd", return_value=str(root)):
                    result = _MOD.read_pane_from_toml(TEST_AGENT, TEST_TEAM)
        self.assertEqual(result.error_code, ERR_FILE_MISSING)


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
            result = _MOD.read_pane_from_roster(TEST_AGENT, TEST_TEAM, {"sender": "team-lead"})
        self.assertEqual(result.pane_id, "%17")
        self.assertEqual(result.source_path, "atm members --team <team> --json")

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
            patch.object(_MOD, "read_pane_from_toml") as mock_toml,
            patch.object(_MOD, "resolve_team", return_value=TEST_TEAM),
        ):
            rc = _MOD.main(["atm-nudge.py", "--pane", "%1", TEST_AGENT, "<atm/>"])
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", TEST_AGENT, "<atm/>")
        mock_roster.assert_not_called()
        mock_toml.assert_not_called()

    def test_override_without_message_builds_default(self):
        with (
            patch.object(_MOD, "nudge_pane") as mock_nudge,
            patch.object(_MOD, "resolve_team", return_value=TEST_TEAM),
            patch.object(_MOD, "read_post_send_payload", return_value={}),
            patch.object(_MOD, "read_pane_from_roster"),
            patch.object(_MOD, "read_pane_from_toml"),
        ):
            rc = _MOD.main(["atm-nudge.py", "--pane", "%1", TEST_AGENT])
        self.assertEqual(rc, 0)
        _, recipient, message = mock_nudge.call_args[0]
        self.assertEqual(recipient, TEST_AGENT)
        self.assertIn(f"read atm --team {TEST_TEAM}", message)


class TestBuildMessage(unittest.TestCase):
    def test_default_send_message_requests_assigned_task_execution(self):
        message = _MOD.build_message(TEST_TEAM, {})
        self.assertIn(f"read atm --team {TEST_TEAM}", message)
        self.assertIn("execute the assigned task", message)
        self.assertIn('busy="after-current-task"', message)
        self.assertNotIn("message ", message)

    def test_send_message_includes_message_id_when_present(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {"message_id": "01JSENDTEST0000000000000000"},
        )
        self.assertIn("<message-id>01JSENDTEST0000000000000000</message-id>", message)
        self.assertIn("execute the assigned task", message)

    def test_send_message_includes_description_when_present(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {
                "message_id": "01JSENDTEST0000000000000000",
                "summary": "review failing smoke lane",
            },
        )
        self.assertIn("<message-id>01JSENDTEST0000000000000000</message-id>", message)
        self.assertIn("<description>review failing smoke lane</description>", message)

    def test_ack_message_requests_immediate_work_with_message_context(self):
        message = _MOD.build_message(
            TEST_TEAM,
            {"is_ack": True, "message_id": "01JACKTEST00000000000000000"},
        )
        self.assertIn(f"read atm --team {TEST_TEAM}", message)
        self.assertIn("<message-id>01JACKTEST00000000000000000</message-id>", message)
        self.assertIn("message 01JACKTEST00000000000000000 acknowledged", message)
        self.assertIn("complete associated work immediately", message)
        self.assertIn(
            'busy="complete tasks based on established priority"',
            message,
        )
        self.assertNotIn("execute the assigned task", message)


class TestMainBehavior(unittest.TestCase):
    def test_roster_match_nudges_without_warning(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup("%1", None, None, "atm members --team <team> --json"),
            PaneLookup("%1", None, None, "/repo/.atm.toml"),
        )
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", TEST_AGENT, unittest.mock.ANY)
        self.assertEqual(stderr_json, {})
        self.assertEqual(stdout_json, {})

    def test_toml_fallback_still_nudges_and_warns(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup(None, ERR_EMPTY_PANE, "missing roster pane", "atm members --team <team> --json"),
            PaneLookup("%1", None, None, "/repo/.atm.toml"),
        )
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%1", TEST_AGENT, unittest.mock.ANY)
        self.assertEqual(stderr_json["status"], "warning")
        self.assertIn(".atm.toml fallback", " ".join(stderr_json["call_to_action"]))
        self.assertIn("SQLite-backed pane metadata", " ".join(stderr_json["call_to_action"]))
        self.assertIn("--pane %1", stderr_json["nudge_command"])
        self.assertEqual(stderr_json["pane_resolution"]["delivered_pane"], "%1")
        self.assertEqual(stdout_json["level"], "warn")
        self.assertEqual(stdout_json["fields"]["delivered_pane"], "%1")
        self.assertEqual(stdout_json["fields"]["roster_error_code"], ERR_EMPTY_PANE)

    def test_roster_command_failure_still_uses_toml_fallback(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_QM],
            PaneLookup(None, ERR_COMMAND_FAILED, "atm members failed", "atm members --team <team> --json"),
            PaneLookup("%2", None, None, "/repo/.atm.toml"),
        )
        self.assertEqual(rc, 0)
        mock_nudge.assert_called_once_with("%2", TEST_QM, unittest.mock.ANY)
        self.assertEqual(stderr_json["status"], "warning")
        self.assertIn("already sent to pane %2", " ".join(stderr_json["call_to_action"]))
        self.assertTrue(any("atm members --team test-team --json" in item for item in stderr_json["fix"]))
        self.assertEqual(stdout_json["fields"]["roster_error_code"], ERR_COMMAND_FAILED)

    def test_roster_and_toml_failure_emit_manual_nudge_and_fix_call_to_action(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_QM],
            PaneLookup(None, ERR_EMPTY_PANE, "bad roster", "atm members --team <team> --json"),
            PaneLookup(None, ERR_PARSE_ERROR, "bad toml", "/repo/.atm.toml"),
        )
        self.assertEqual(rc, 1)
        mock_nudge.assert_not_called()
        self.assertEqual(stdout_json, {})
        self.assertEqual(stderr_json["status"], "error")
        self.assertIn("Run nudge_command NOW", " ".join(stderr_json["call_to_action"]))
        self.assertIn("VERIFY the pane id", " ".join(stderr_json["call_to_action"]))
        self.assertIn(f"--pane {CODEX_DEFAULT_PANE}", stderr_json["nudge_command"])
        self.assertIn("Repair canonical ATM roster pane metadata", " ".join(stderr_json["fix"]))
        self.assertIn("Fix or restore the repo-local .atm.toml", " ".join(stderr_json["fix"]))

    def test_neither_source_found_uses_default_pane(self):
        rc, stderr_json, stdout_json, mock_nudge = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup(None, ERR_NOT_FOUND, "missing member", "atm members --team <team> --json"),
            PaneLookup(None, ERR_NOT_FOUND, "missing recipient", "/repo/.atm.toml"),
        )
        self.assertEqual(rc, 1)
        mock_nudge.assert_not_called()
        self.assertEqual(stdout_json, {})
        self.assertIn(f"--pane {CODEX_DEFAULT_PANE}", stderr_json["nudge_command"])
        self.assertIn("VERIFY the pane id", " ".join(stderr_json["call_to_action"]))

    def test_error_payload_includes_input_and_resolution_context(self):
        rc, stderr_json, _, _ = _run_with_mocked_lookups(
            [TEST_AGENT],
            PaneLookup(None, ERR_FILE_MISSING, "missing", "atm members --team <team> --json"),
            PaneLookup(None, ERR_FILE_MISSING, "missing", None),
        )
        self.assertEqual(rc, 1)
        self.assertIn("input", stderr_json)
        self.assertIn("pane_resolution", stderr_json)
        self.assertEqual(stderr_json["input"]["recipient"], TEST_AGENT)
        self.assertEqual(stderr_json["pane_resolution"]["authoritative_source"], "atm roster")


if __name__ == "__main__":
    unittest.main()

"""Unit tests for the progressive feature smoke dispatcher."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest


def load_runner():
    path = Path(__file__).with_name("run_feature_smoke.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("run_feature_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


class FeatureSmokeTests(unittest.TestCase):
    def test_local_ip_alias_is_supported(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "local-up"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("local-ip", [])

    def test_crosshost_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost", "m5", "fastpc4"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost-send", ["m5", "fastpc4"])

    def test_crosshost_ack_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost-ack", "m5"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost-ack", ["m5"])

    def test_crosshost_feature_requires_a_peer(self):
        with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost-send"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "requires one or more SSH hostnames"):
                RUNNER.main()

    def test_fixture_level_retains_existing_runner(self):
        completed = mock.Mock(returncode=0)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "thorough"]):
                self.assertEqual(RUNNER.main(), 0)
        self.assertEqual(Path(run.call_args.args[0][1]).name, "run.py")

    def test_missing_identity_is_a_hard_failure(self):
        with mock.patch.dict(os.environ, {"ATM_IDENTITY": "", "ATM_TEAM": ""}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_IDENTITY"):
                RUNNER.require_environment()

    def test_branch_version_requires_one_shared_cli_daemon_version(self):
        metadata = {"packages": [{"name": "atm", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.27"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            self.assertEqual(RUNNER.branch_version(), "1.3.2-beta.27")

    def test_branch_version_rejects_divergent_cli_daemon_versions(self):
        metadata = {"packages": [{"name": "atm", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.28"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            with self.assertRaisesRegex(RUNNER.SmokeError, "shared"):
                RUNNER.branch_version()

    def test_remote_hosts_are_rejected_for_localhost_feature(self):
        with mock.patch.object(RUNNER.sys, "argv", ["smoke", "localhost", "m5"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "only valid"):
                RUNNER.main()

    def test_report_writes_browser_frame_for_xhtml_pane(self):
        with tempfile.TemporaryDirectory() as temp:
            with mock.patch.dict(os.environ, {"ATM_SMOKE_RUN_ID": "smoke-42"}, clear=False):
                with mock.patch.object(RUNNER, "ROOT", Path(temp)):
                    with mock.patch.object(RUNNER, "compose") as compose:
                        report = RUNNER.write_report("localhost", [{"name": "doctor", "status": "PASS", "detail": "ready"}])
        self.assertEqual(compose.call_count, 3)
        self.assertEqual(compose.call_args_list[1].args[2], report.with_suffix(".html"))
        self.assertEqual(compose.call_args_list[2].args[2], report.parent / "index.html")

    def test_feature_pane_renders_each_executed_case(self):
        pane = RUNNER.render_feature_pane(
            "localhost",
            [
                {"name": "doctor", "status": "PASS", "detail": "status: healthy\nreadiness: ready"},
                {"name": "localhost send/read", "status": "PASS", "detail": "01TEST"},
            ],
        )
        self.assertIn("localhost send/read", pane)
        self.assertIn("Doctor passed", pane)
        self.assertIn("healthy<br />readiness", pane)
        self.assertNotIn("<td>doctor</td>", pane)

    def test_artifact_segment_rejects_path_traversal(self):
        with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_SMOKE_RUN_ID"):
            RUNNER.artifact_segment("../other-run", "ATM_SMOKE_RUN_ID")

    def test_ack_reply_contract_requires_a_sent_reply_ulid(self):
        self.assertEqual(
            RUNNER.reply_message_id(
                {"reply_disposition": {"kind": "sent", "reply_message_id": "01TEST"}}
            ),
            "01TEST",
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "sent reply"):
            RUNNER.reply_message_id({"reply_disposition": {"kind": "failed"}})

    def test_message_has_text_requires_exact_body(self):
        self.assertTrue(RUNNER.message_has_text({"text": "exact"}, "exact"))
        self.assertFalse(RUNNER.message_has_text({"text": "different"}, "exact"))

    def test_doctor_ready_requires_health_readiness_and_matching_pair(self):
        report = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.3.2-beta.28"},
            "daemon_context": {"version": "1.3.2-beta.28"},
        }
        self.assertTrue(RUNNER.doctor_ready(report, "1.3.2-beta.28"))
        report["daemon_context"]["version"] = "1.3.2-beta.27"
        self.assertFalse(RUNNER.doctor_ready(report, "1.3.2-beta.28"))

    def test_advertised_host_from_json_requires_enabled_host(self):
        self.assertEqual(
            RUNNER.advertised_host_from_json(
                {"interfaces": [{"enabled": False, "advertise_host": "old"}, {"enabled": True, "advertise_host": "m5.local"}]}
            ),
            "m5.local",
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "no enabled"):
            RUNNER.advertised_host_from_json({"interfaces": [{"enabled": False, "advertise_host": "old"}]})

    def test_crosshost_send_requires_remote_exact_ulid_and_body(self):
        sent = {"message_id": "01SEND"}
        remote_read = {"message": {"message_id": "01SEND", "text": "smoke-crosshost-send-m5-STAMP"}}
        cases = []

        class Clock:
            @staticmethod
            def now(_timezone):
                return type("FixedTime", (), {"strftime": lambda self, _format: "STAMP"})()

        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(sent), "stderr": ""}), mock.patch.object(
            RUNNER, "remote_command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(remote_read), "stderr": ""}
        ), mock.patch.object(RUNNER, "datetime", Clock):
            RUNNER.crosshost_send(cases, "atm", "atm", "agent", "team", "m5", "m5.local")
        self.assertEqual(cases[-1]["status"], "PASS")
        self.assertEqual(cases[-1]["detail"], "01SEND")

    def test_crosshost_ack_verifies_reverse_reply_linkage(self):
        sent = {"message_id": "01SEND"}
        remote_read = {
            "message": {
                "message_id": "01SEND",
                "text": "smoke-crosshost-ack-m5-STAMP",
                "requires_ack": True,
            }
        }
        remote_ack = {"reply_disposition": {"kind": "sent", "reply_message_id": "01REPLY"}}
        cases = []

        class Clock:
            @staticmethod
            def now(_timezone):
                return type("FixedTime", (), {"strftime": lambda self, _format: "STAMP"})()

        def local_command(argv, timeout=15.0):
            return {"exit_code": 0, "stdout": __import__("json").dumps(sent), "stderr": ""}

        def remote(peer, atm, args, timeout=20.0):
            if args[0] == "read":
                return {"exit_code": 0, "stdout": __import__("json").dumps(remote_read), "stderr": ""}
            return {"exit_code": 0, "stdout": __import__("json").dumps(remote_ack), "stderr": ""}

        def local_reply(atm, team, expected, timeout=12.0):
            return {
                "message_id": expected,
                "text": "smoke-crosshost-reply-m5-STAMP",
                "acknowledgesMessageId": "01SEND",
            }

        with mock.patch.object(RUNNER, "command", side_effect=local_command), mock.patch.object(
            RUNNER, "remote_command", side_effect=remote
        ), mock.patch.object(RUNNER, "wait_for_message", side_effect=local_reply), mock.patch.object(
            RUNNER, "datetime", Clock
        ):
            RUNNER.crosshost_ack(cases, "atm", "atm", "agent", "team", "m5", "m5.local")
        self.assertEqual([case["status"] for case in cases], ["PASS", "PASS"])
        self.assertEqual(cases[-1]["detail"], "01REPLY")


if __name__ == "__main__":
    unittest.main()

"""Unit tests for the inbound peer smoke runner; no daemon or SSH required."""
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from unittest import mock
from pathlib import Path


def load_runner():
    path = Path(__file__).with_name("run_inbound_peer_smoke.py")
    spec = importlib.util.spec_from_file_location("run_inbound_peer_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
TEST_VERSION = "1.3.2-beta.27"


class InboundPeerSmokeTests(unittest.TestCase):
    def test_extract_message_id_handles_nested_send_output(self):
        self.assertEqual(RUNNER.extract_message_id('{"result":{"message_id":"01TEST"}}'), "01TEST")

    def test_advertised_host_finds_enabled_interface(self):
        raw = '{"interfaces":[{"enabled":false,"advertise_host":"old"},{"enabled":true,"advertise_host":"peer.local"}]}'
        self.assertEqual(RUNNER.extract_advertised_host(raw), "peer.local")

    def test_posix_remote_command_preserves_environment_without_local_shell(self):
        command = RUNNER.remote_command(
            {"ssh_command": ["ssh", "m5"], "shell": "posix"},
            ["atm", "send", "a@b.host", "hello world", "--json"],
            {"ATM_IDENTITY": "cm5", "ATM_TEAM": "atm-m5"},
        )
        self.assertEqual(command[:4], ["ssh", "m5", "sh", "-lc"])
        self.assertIn("ATM_IDENTITY=cm5", command[-1])
        self.assertIn("'hello world'", command[-1])

    def test_powershell_remote_command_sets_environment(self):
        command = RUNNER.remote_command(
            {"ssh_command": ["ssh", "fastpc4"], "shell": "powershell"},
            ["C:\\atm.exe", "doctor", "--json"],
            {"ATM_IDENTITY": "windows-smoke"},
        )
        self.assertEqual(command[:6], ["ssh", "fastpc4", "powershell", "-NoProfile", "-NonInteractive", "-Command"])
        self.assertIn("$env:ATM_IDENTITY='windows-smoke'", command[-1])

    def test_read_state_requires_exact_message_and_pending_ack(self):
        result = {"message": {"message_id": "01TEST", "requires_ack": True}}
        self.assertEqual(RUNNER.message_from_read('{"message":{"message_id":"01TEST","requires_ack":true}}'), result["message"])
        self.assertIsNone(RUNNER.message_from_read("not json"))

    def test_xhtml_escapes_host_and_marks_not_run(self):
        pane = RUNNER.render_host_pane(
            "m5 <unsafe>",
            {"exit_code": 0, "stdout": "{}", "stderr": ""},
            {"doctor": ("pass", "ready"), "nudge": ("not-run", "not configured")},
            [],
        )
        self.assertIn("m5 &lt;unsafe&gt;", pane)
        self.assertIn('class="not-run"', pane)
        self.assertIn("No executed failure. Remaining investigation: nudge", pane)

    def test_xhtml_marks_failed_phase_red(self):
        pane = RUNNER.render_host_pane(
            "local", None, {"doctor": ("fail", "daemon unavailable")}, [{"phase": "local-doctor", "passed": False}],
        )
        self.assertIn('class="fail"', pane)
        self.assertIn("Investigation required: local-doctor", pane)

    def test_doctor_version_or_api_mismatch_is_a_hard_failure(self):
        local = {"expected_daemon_version": TEST_VERSION, "expected_http_api_version": 1}
        result = {"exit_code": 0, "stderr": "", "stdout": json.dumps({
            "daemon_context": {"version": "1.3.1"},
            "daemon_runtime": {"http_api_version": 1, "peer_wire_security": "mutual_tls"},
        })}
        passed, detail = RUNNER.doctor_matches_expected(local, result)
        self.assertFalse(passed)
        self.assertIn("daemon version", detail)

    def test_host_mode_accepts_no_ssh_peers_with_required_local_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "host.json"
            path.write_text(json.dumps({
                "schema_version": 1,
                "local": {"atm_command": ["atm"], "identity": "a", "team": "t", "expected_daemon_version": TEST_VERSION, "expected_http_api_version": 1},
                "host": {"name": "m5", "local_checks": {
                    "localhost/local loopback": ["loopback"],
                    "own-IP": ["own-ip"],
                    "nudge": ["nudge"],
                }},
            }), encoding="utf-8")
            config = RUNNER.load_config(path)
        self.assertEqual(RUNNER.validate_host_config(config)["name"], "m5")

    def test_host_mode_rejects_missing_required_local_check(self):
        config = {
            "host": {
                "name": "m5",
                "local_checks": {
                    "localhost/local loopback": ["loopback"],
                    "own-IP": ["own-ip"],
                },
            },
        }
        with self.assertRaisesRegex(RUNNER.SmokeError, "nudge"):
            RUNNER.validate_host_config(config)

    def test_handoff_requires_exact_ids_and_known_kinds(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "handoff.json"
            path.write_text(json.dumps({"host": "m5", "outbound": [
                {"kind": "remote incoming no-ack", "message_id": "01NOACK"},
                {"kind": "remote incoming requires-ack", "message_id": "01ACK"},
            ]}), encoding="utf-8")
            self.assertEqual(RUNNER.load_handoff(path), ("m5", [
                {"kind": "remote incoming no-ack", "message_id": "01NOACK"},
                {"kind": "remote incoming requires-ack", "message_id": "01ACK"},
            ]))

    def test_default_run_executes_declared_local_preflight_rows(self):
        config = {
            "schema_version": 1,
            "local": {
                "atm_command": ["atm"], "identity": "a", "team": "t",
                "expected_daemon_version": TEST_VERSION, "expected_http_api_version": 1,
                "advertised_host": "127.0.0.1",
            },
            "host": {
                "name": "local",
                "local_checks": {
                    "localhost/local loopback": ["check-loopback"],
                    "own-IP": ["check-own-ip"],
                    "nudge": ["check-nudge"],
                },
            },
            "peers": [],
        }
        doctor = json.dumps({
            "daemon_context": {"version": TEST_VERSION},
            "daemon_runtime": {"http_api_version": 1, "peer_wire_security": "mutual_tls"},
        })

        def command_result(command, _timeout):
            return {"command": command, "exit_code": 0, "stdout": doctor if command[-2:] == ["doctor", "--json"] else "", "stderr": ""}

        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(RUNNER, "command_result", side_effect=command_result), \
             mock.patch.object(RUNNER, "compose"):
            self.assertEqual(RUNNER.run(config, Path(directory), 1, 1), 0)
            results = next(Path(directory).glob("*/results.json"))
            phases = {item["phase"] for item in json.loads(results.read_text(encoding="utf-8"))["records"]}
        self.assertTrue({"localhost/local loopback", "own-IP", "nudge"}.issubset(phases))


if __name__ == "__main__":
    unittest.main()

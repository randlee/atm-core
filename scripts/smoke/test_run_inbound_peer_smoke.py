"""Unit tests for the inbound peer smoke runner; no daemon or SSH required."""
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


def load_runner():
    path = Path(__file__).with_name("run_inbound_peer_smoke.py")
    spec = importlib.util.spec_from_file_location("run_inbound_peer_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


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

    def test_host_mode_accepts_no_ssh_peers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "host.json"
            path.write_text(json.dumps({
                "schema_version": 1,
                "local": {"atm_command": ["atm"], "identity": "a", "team": "t"},
                "host": {"name": "m5", "local_checks": {}},
            }), encoding="utf-8")
            config = RUNNER.load_config(path)
        self.assertEqual(RUNNER.validate_host_config(config)["name"], "m5")


if __name__ == "__main__":
    unittest.main()

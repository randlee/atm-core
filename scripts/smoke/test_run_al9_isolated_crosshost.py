"""Unit coverage for the AL.9 isolated cross-host proof runner."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock


def load_runner():
    path = Path(__file__).with_name("run_al9_isolated_crosshost.py")
    spec = importlib.util.spec_from_file_location("run_al9_isolated_crosshost", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


def side(identity: str) -> dict[str, object]:
    return {
        "atm_command": ["/clean/atm"],
        "preflight_command": ["/clean/verify-atm-http-runtime"],
        "revision": "al9-proof-sha",
        "identity": identity,
        "team": "atm-dev",
        "environment": {"ATM_HTTP_RUNTIME": "isolated", "ATM_HOME": f"/clean/{identity}"},
    }


class IsolatedCrossHostProofTests(unittest.TestCase):
    def test_config_rejects_missing_isolated_environment(self):
        config = {"schema_version": 1, "sender": side("sender"), "receiver": side("receiver"), "recipient": "receiver@atm-dev.peer"}
        config["sender"]["environment"] = {}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "proof.json"
            path.write_text(json.dumps(config), encoding="utf-8")
            with self.assertRaisesRegex(RUNNER.SmokeError, "environment"):
                RUNNER.load_config(path)

    def test_commands_only_use_explicit_replacement_atm_and_no_shell(self):
        sender = side("sender")
        command = RUNNER.send_command(sender, "receiver@atm-dev.peer", "body with spaces")
        self.assertEqual(command[0], "env")
        self.assertIn("ATM_HTTP_RUNTIME=isolated", command)
        self.assertIn("/clean/atm", command)
        self.assertIn("send", command)
        self.assertNotIn("sh", command)

    def test_remote_command_quotes_the_isolated_environment(self):
        sender = side("sender")
        sender["ssh_command"] = ["ssh", "m5"]
        command = RUNNER.send_command(sender, "receiver@atm-dev.peer", "body with spaces")
        self.assertEqual(command[:4], ["ssh", "m5", "sh", "-lc"])
        self.assertIn("ATM_HTTP_RUNTIME=isolated", command[-1])
        self.assertIn("'body with spaces'", command[-1])

    def test_preflight_rejects_legacy_runtime_or_revision_mismatch(self):
        with self.assertRaisesRegex(RUNNER.SmokeError, "not atm-http-runtime"):
            RUNNER.parse_replacement_preflight(json.dumps({"replacement_runtime": "atm-daemon", "revision": "sha"}), "sha")
        with self.assertRaisesRegex(RUNNER.SmokeError, "expected"):
            RUNNER.parse_replacement_preflight(json.dumps({"replacement_runtime": "atm-http-runtime", "revision": "wrong"}), "sha")

    def test_config_rejects_cross_host_revision_mismatch(self):
        config = {"schema_version": 1, "sender": side("sender"), "receiver": side("receiver"), "recipient": "receiver@atm-dev.peer"}
        config["receiver"]["revision"] = "different-sha"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "proof.json"
            path.write_text(json.dumps(config), encoding="utf-8")
            with self.assertRaisesRegex(RUNNER.SmokeError, "same replacement revision"):
                RUNNER.load_config(path)

    def test_run_records_route_storage_and_hook_evidence(self):
        config = {"sender": side("sender"), "receiver": side("receiver"), "recipient": "receiver@atm-dev.peer"}
        outputs = iter([
            json.dumps({"replacement_runtime": "atm-http-runtime", "revision": "al9-proof-sha", "route_evidence": "client"}),
            json.dumps({"replacement_runtime": "atm-http-runtime", "revision": "al9-proof-sha", "storage_evidence": "stored", "hook_evidence": "once"}),
            json.dumps({"message_id": "01TEST"}),
            json.dumps({"message": {"message_id": "01TEST"}}),
        ])

        def completed(command):
            return __import__("subprocess").CompletedProcess(command, 0, next(outputs), "")

        with mock.patch.object(RUNNER, "result", side_effect=completed):
            evidence = RUNNER.run(config, "proof body")
        self.assertEqual(evidence["message_id"], "01TEST")
        self.assertEqual(evidence["sender_route"], "client")
        self.assertEqual(evidence["receiver_storage"], "stored")
        self.assertEqual(evidence["receiver_hook"], "once")


if __name__ == "__main__":
    unittest.main()

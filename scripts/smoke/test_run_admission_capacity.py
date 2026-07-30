"""Focused unit tests for the AI.33 public admission-capacity runner."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import socket
import sys
import tempfile
import unittest
from unittest import mock


def load_runner():
    path = Path(__file__).with_name("run_admission_capacity.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("run_admission_capacity", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


class AdmissionCapacityTests(unittest.TestCase):
    def temp_path(self, name: str) -> Path:
        return Path(tempfile.gettempdir()) / name

    def test_home_rejects_production_or_non_temporary_paths(self):
        account_home = self.temp_path("capacity-account")
        with mock.patch.object(RUNNER, "os_account_home", return_value=account_home):
            with self.assertRaisesRegex(RUNNER.SmokeError, "production"):
                RUNNER.validate_capacity_home(account_home / ".atm")
        with self.assertRaisesRegex(RUNNER.SmokeError, "basename"):
            RUNNER.validate_capacity_home(Path(tempfile.gettempdir()) / "shared-atm")

    def test_home_accepts_only_a_marked_temporary_directory(self):
        path = Path(tempfile.gettempdir()) / "atm-capacity-unit-home"
        self.assertEqual(RUNNER.validate_capacity_home(path), path.resolve())

    def test_requires_explicit_clean_os_user_guard(self):
        with mock.patch.dict(os.environ, {"ATM_CAPACITY_ISOLATED_OS_USER": ""}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "dedicated clean OS-user"):
                RUNNER.require_isolated_os_user()

    def test_public_request_is_host_qualified_and_never_a_dispatch_envelope(self):
        body = __import__("json").loads(RUNNER.http_request_body(self.temp_path("atm-capacity-test"), 42, "192.0.2.10"))
        self.assertEqual(body["to"], {"agent": "capacity-agent", "team": "capacity-team", "host": "192.0.2.10"})
        self.assertEqual(body["message_source"], {"Inline": "capacity-42"})
        self.assertNotIn("RequestEnvelope", body)

    def test_controlled_peer_configuration_uses_the_public_cli_reload_path(self):
        result = {"exit_code": 0, "stdout": "", "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=result) as command:
            RUNNER.configure_controlled_peer(
                self.temp_path("atm"), {"ATM_HOME": str(self.temp_path("atm-capacity-test"))}, "192.0.2.10", "fingerprint"
            )
        self.assertEqual(
            command.call_args.args[0],
            [
                str(self.temp_path("atm")), "peer", "trust", "add", "--host", "192.0.2.10",
                "--fingerprint", "fingerprint", "--yes",
            ],
        )

    def test_interval_preserves_the_first_failure_and_requires_all_1000_responses(self):
        calls = 0

        def submit(_sequence):
            nonlocal calls
            calls += 1
            return RUNNER.AdmissionResult(201 if calls != 7 else 503, 0.1, None if calls != 7 else "HTTP 503")

        with mock.patch.object(RUNNER, "ADMISSIONS_PER_INTERVAL", 10), mock.patch.object(RUNNER, "WORKERS", 2):
            result = RUNNER.run_interval(submit, 0)
        self.assertEqual(result["accepted_count"], 9)
        self.assertEqual(result["response_count"], 10)
        self.assertEqual(result["first_failure"], "HTTP 503")
        self.assertFalse(result["passed"])

    def test_peer_case_retains_each_interval_in_evidence(self):
        with mock.patch.object(RUNNER, "INTERVALS", 3), mock.patch.object(
            RUNNER, "run_interval", return_value={"passed": True}
        ) as interval:
            result = RUNNER.run_peer_case(
                RUNNER.LocalEndpoint("uds", str(self.temp_path("socket"))), self.temp_path("atm-capacity-test"), "peer.example", "peer"
            )
        self.assertEqual(len(result["intervals"]), 3)
        self.assertTrue(result["passed"])
        self.assertEqual(interval.call_count, 3)

    def test_response_summary_preserves_structured_daemon_error(self):
        client, daemon = socket.socketpair()
        self.addCleanup(client.close)
        self.addCleanup(daemon.close)
        payload = b'{"code":"ATM_DAEMON_CONNECTION_SATURATED","message":"local admission saturated"}'
        daemon.sendall(
            b"HTTP/1.1 503 Service Unavailable\r\n"
            + f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii")
            + payload
        )
        response = RUNNER.read_http_response_summary(client)
        self.assertEqual(response.status, 503)
        self.assertEqual(response.failure, "HTTP 503 ATM_DAEMON_CONNECTION_SATURATED: local admission saturated")

    def test_feature_report_reuses_the_unified_html_report_writer(self):
        evidence = {
            "doctor": {"summary": {"status": "healthy"}},
            "runs": [{"label": "accepting", "passed": False, "intervals": [{"accepted_count": 999, "first_failure": "HTTP 503 TEST"}]}],
        }
        with mock.patch("run_feature_smoke.write_report", return_value=Path("report.json")) as writer:
            report = RUNNER.write_feature_report(evidence)
        self.assertEqual(report, Path("report.json"))
        self.assertEqual(writer.call_args.args[0], "admission-capacity")
        self.assertEqual(writer.call_args.args[1][1]["detail"], "999/1000 accepted; HTTP 503 TEST")


if __name__ == "__main__":
    unittest.main()

"""Focused unit tests for the AI.33 public admission-capacity runner."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
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
    def test_home_rejects_production_or_non_temporary_paths(self):
        with mock.patch.object(RUNNER, "os_account_home", return_value=Path("/Users/capacity")):
            with self.assertRaisesRegex(RUNNER.SmokeError, "temporary"):
                RUNNER.validate_capacity_home(Path("/Users/capacity/.atm"))
        with self.assertRaisesRegex(RUNNER.SmokeError, "basename"):
            RUNNER.validate_capacity_home(Path(tempfile.gettempdir()) / "shared-atm")

    def test_home_accepts_only_a_marked_temporary_directory(self):
        path = Path(tempfile.gettempdir()) / "atm-capacity-unit-home"
        self.assertEqual(RUNNER.validate_capacity_home(path), path.resolve())

    def test_requires_explicit_clean_os_user_guard(self):
        with mock.patch.dict(os.environ, {"ATM_CAPACITY_ISOLATED_OS_USER": ""}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "dedicated clean OS-user"):
                RUNNER.require_isolated_os_user()

    def test_isolation_accepts_clean_user_or_explicit_backup_restore(self):
        with mock.patch.dict(os.environ, {"ATM_CAPACITY_ISOLATED_OS_USER": "1"}, clear=False):
            self.assertEqual(RUNNER.select_host_state_isolation(), "isolated_os_user")
        with mock.patch.dict(
            os.environ,
            {"ATM_CAPACITY_ISOLATED_OS_USER": "", "ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE": "1"},
            clear=False,
        ):
            self.assertEqual(RUNNER.select_host_state_isolation(), "backup_restore")

    def test_backup_restore_returns_the_complete_prior_host_state(self):
        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            original = os_home / ".atm"
            original.mkdir()
            (original / "mail.db").write_text("prior state", encoding="utf-8")
            with mock.patch.object(RUNNER, "os_account_home", return_value=os_home):
                backup = RUNNER.HostStateBackup.begin()
                (os_home / ".atm" / "mail.db").write_text("benchmark state", encoding="utf-8")
                backup.restore()
            self.assertEqual((original / "mail.db").read_text(encoding="utf-8"), "prior state")

    def test_transport_is_platform_explicit(self):
        self.assertEqual(RUNNER.validate_transport("tcp"), "tcp")
        self.assertEqual(RUNNER.validate_transport("uds"), "uds")
        with self.assertRaisesRegex(RUNNER.SmokeError, "must be"):
            RUNNER.validate_transport("https")
        with mock.patch.object(RUNNER.os, "name", "nt"):
            with self.assertRaisesRegex(RUNNER.SmokeError, "Windows"):
                RUNNER.validate_transport("uds")

    def test_sparse_profiles_and_schema_fields_are_declared(self):
        self.assertEqual(RUNNER.SPARSE_FRAMES_PER_CONNECTION, (1, 2, 8, 16, 64))

    def test_response_reader_consumes_declared_body(self):
        class Stream:
            def __init__(self):
                self.chunks = [b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n", b"{}"]

            def recv(self, _size):
                return self.chunks.pop(0) if self.chunks else b""

        status, wire_bytes = RUNNER.read_http_response(Stream())
        self.assertEqual(status, 201)
        self.assertGreater(wire_bytes, 2)

    def test_public_request_is_host_qualified_and_never_a_dispatch_envelope(self):
        body = __import__("json").loads(RUNNER.http_request_body(Path("/tmp/atm-capacity-test"), 42, "192.0.2.10"))
        self.assertEqual(body["to"], {"agent": "capacity-agent", "team": "capacity-team", "host": "192.0.2.10"})
        self.assertEqual(body["message_source"], {"Inline": "capacity-42"})
        self.assertNotIn("RequestEnvelope", body)

    def test_controlled_peer_configuration_uses_the_public_cli_reload_path(self):
        result = {"exit_code": 0, "stdout": "", "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=result) as command:
            RUNNER.configure_controlled_peer(
                Path("/tmp/atm"), {"ATM_HOME": "/tmp/atm-capacity-test"}, "192.0.2.10", "fingerprint"
            )
        self.assertEqual(
            command.call_args.args[0],
            [
                "/tmp/atm", "peer", "trust", "add", "--host", "192.0.2.10",
                "--fingerprint", "fingerprint", "--yes",
            ],
        )

    def test_interval_preserves_the_first_failure_and_requires_all_1000_responses(self):
        calls = 0

        def submit(_sequence, _message_count):
            nonlocal calls
            calls += 1
            return [RUNNER.AdmissionResult(201 if calls != 7 else 503, 0.1, None if calls != 7 else "HTTP 503")]

        with mock.patch.object(RUNNER, "ADMISSIONS_PER_INTERVAL", 10), mock.patch.object(RUNNER, "WORKERS", 2):
            result = RUNNER.run_interval(submit, 0, 1, 10)
        self.assertEqual(result["accepted_count"], 9)
        self.assertEqual(result["response_count"], 10)
        self.assertEqual(result["first_failure"], "HTTP 503")
        self.assertFalse(result["passed"])

    def test_interval_allows_a_partial_final_connection(self):
        requested_connection_sizes: list[int] = []

        def submit(_sequence, message_count):
            requested_connection_sizes.append(message_count)
            return [RUNNER.AdmissionResult(201, 0.1) for _ in range(message_count)]

        with mock.patch.object(RUNNER, "WORKERS", 2):
            result = RUNNER.run_interval(submit, 0, 64, 1_000)
        self.assertEqual(result["accepted_count"], 1_000)
        self.assertEqual(result["connections"], 16)
        self.assertEqual(sorted(requested_connection_sizes)[0], 40)
        self.assertTrue(result["passed"])

    def test_profile_retains_each_requested_interval_in_evidence(self):
        with mock.patch.object(
            RUNNER, "run_interval", return_value={"passed": True}
        ) as interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"),
                "peer.example",
                2,
                10_000,
                3,
            )
        self.assertEqual(len(result["intervals"]), 3)
        self.assertTrue(result["passed"])
        self.assertEqual(interval.call_count, 3)
        self.assertEqual(interval.call_args.args[2:], (2, 10_000))


if __name__ == "__main__":
    unittest.main()

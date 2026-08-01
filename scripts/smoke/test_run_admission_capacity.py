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

    def test_profile_selection_places_sparse_samples_before_sustained_profiles(self):
        self.assertEqual(
            RUNNER.selected_profiles((1, 8), (10_000, 100_000)),
            (
                (1, 1_000),
                (8, 1_000),
                (1, 10_000),
                (8, 10_000),
                (1, 100_000),
                (8, 100_000),
            ),
        )

    def test_evidence_file_retains_the_transport_schema_fields(self):
        evidence = {
            "schema_version": 2,
            "host_label": "test-host",
            "transport": "tcp",
            "frames_per_connection": 16,
            "requested_messages_per_sample": 1_000,
            "run_duration_s": 1.25,
        }
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            recorded = __import__("json").loads(path.read_text(encoding="utf-8"))

        self.assertEqual(recorded, evidence)

    def test_response_reader_consumes_declared_body(self):
        class Stream:
            def __init__(self):
                self.chunks = [b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n", b"{}"]

            def recv(self, _size):
                return self.chunks.pop(0) if self.chunks else b""

        status, wire_bytes, summary = RUNNER.read_http_response(Stream())
        self.assertEqual(status, 201)
        self.assertGreater(wire_bytes, 2)
        self.assertIsNone(summary)

    def test_response_reader_retains_a_bounded_error_body(self):
        class Stream:
            def __init__(self):
                self.chunks = [b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 5\r\n\r\nerror"]

            def recv(self, _size):
                return self.chunks.pop(0) if self.chunks else b""

        status, _wire_bytes, summary = RUNNER.read_http_response(Stream())
        self.assertEqual(status, 503)
        self.assertEqual(summary, "error")

    def test_response_reader_preserves_the_next_pipelined_response(self):
        class Stream:
            def __init__(self):
                self.chunks = [
                    b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n{}"
                    b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n{}"
                ]

            def recv(self, _size):
                return self.chunks.pop(0) if self.chunks else b""

        buffered = bytearray()
        stream = Stream()
        first = RUNNER.read_http_response(stream, buffered)
        second = RUNNER.read_http_response(stream, buffered)
        self.assertEqual(first[0], 201)
        self.assertEqual(second[0], 201)
        self.assertEqual(buffered, b"")

    def test_public_request_targets_a_distinct_local_recipient(self):
        body = __import__("json").loads(RUNNER.http_request_body(Path("/tmp/atm-capacity-test"), 42))
        self.assertEqual(body["to"], {"agent": "capacity-recipient", "team": "capacity-team"})
        self.assertEqual(body["message_source"], {"Inline": "capacity-42"})
        self.assertNotIn("RequestEnvelope", body)

    def test_capacity_roster_creates_sender_and_distinct_local_recipient(self):
        result = {"exit_code": 0, "stdout": "", "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=result) as command:
            RUNNER.prepare_capacity_roster(
                Path("/tmp/atm"), {"ATM_HOME": "/tmp/atm-capacity-test"}, Path("/tmp/capacity-home")
            )
        self.assertEqual(
            command.call_args_list[0].args[0],
            [
                "/tmp/atm", "teams", "add-member", "capacity-team", "capacity-agent",
                "--home-dir", "/tmp/capacity-home", "--json",
            ],
        )
        self.assertEqual(command.call_args_list[1].args[0][4], "capacity-recipient")

    def test_interval_preserves_the_first_failure_and_requires_all_1000_responses(self):
        calls = 0

        def submit(_sequence, _message_count):
            nonlocal calls
            calls += 1
            return [RUNNER.AdmissionResult(201 if calls != 7 else 503, 0.1, None if calls != 7 else "HTTP 503")]

        with mock.patch.object(RUNNER, "ADMISSIONS_PER_INTERVAL", 10):
            result = RUNNER.run_interval(submit, 0, 1, 2, 10)
        self.assertEqual(result["accepted_count"], 9)
        self.assertEqual(result["response_count"], 10)
        self.assertEqual(result["first_failure"], "HTTP 503")
        self.assertFalse(result["passed"])

    def test_interval_allows_a_partial_final_connection(self):
        requested_connection_sizes: list[int] = []

        def submit(_sequence, message_count):
            requested_connection_sizes.append(message_count)
            return [RUNNER.AdmissionResult(201, 0.1) for _ in range(message_count)]

        result = RUNNER.run_interval(submit, 0, 64, 2, 1_000)
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
                2,
                10_000,
                3,
                2,
            )
        self.assertEqual(len(result["intervals"]), 3)
        self.assertTrue(result["passed"])
        self.assertEqual(interval.call_count, 3)
        self.assertEqual(interval.call_args.args[2:], (2, 2, 10_000))

    def test_runner_reaps_its_owned_daemon_after_signal(self):
        process = mock.Mock()
        process.pid = 42
        process.wait.return_value = 0
        with mock.patch.object(RUNNER, "terminate_process") as terminate:
            # The runner must use Popen.wait(), not pid probing: an exited child
            # is a zombie until its owner reaps it.
            RUNNER.reap_owned_daemon(process)
        terminate.assert_called_once_with(42)
        process.wait.assert_called_once_with(timeout=10.0)

    def test_daemon_output_capture_retains_bounded_stdout_and_stderr_tails(self):
        capture = RUNNER.DaemonOutputCapture()
        for index in range(RUNNER.DAEMON_OUTPUT_TAIL_LINES + 2):
            capture._append_tail(capture._stdout_tail, f"stdout-{index}\n")
            capture._append_tail(capture._stderr_tail, f"stderr-{index}\n")

        evidence = capture.evidence()
        self.assertEqual(len(evidence["stdout_tail"]), RUNNER.DAEMON_OUTPUT_TAIL_LINES)
        self.assertEqual(len(evidence["stderr_tail"]), RUNNER.DAEMON_OUTPUT_TAIL_LINES)
        self.assertEqual(evidence["stdout_tail"][0], "stdout-2")
        self.assertEqual(
            evidence["stderr_tail"][-1],
            f"stderr-{RUNNER.DAEMON_OUTPUT_TAIL_LINES + 1}",
        )


if __name__ == "__main__":
    unittest.main()

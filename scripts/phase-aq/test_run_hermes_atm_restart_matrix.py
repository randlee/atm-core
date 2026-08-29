from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
from typing import Any
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("run_hermes_atm_restart_matrix.py")


def load_module():
    spec = importlib.util.spec_from_file_location("run_hermes_atm_restart_matrix", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class HermesAtmRestartMatrixTests(unittest.TestCase):
    def test_refresh_sender_after_daemon_restart_is_windows_only(self) -> None:
        module = load_module()

        class Sender:
            def __init__(self) -> None:
                self.reconnect_calls = 0

            def reconnect(self) -> None:
                self.reconnect_calls += 1

        windows_sender = Sender()
        windows_record: dict[str, Any] = {}
        module.refresh_sender_after_daemon_restart(
            windows_sender,
            windows_record,
            platform_name="nt",
        )
        self.assertEqual(windows_sender.reconnect_calls, 1)
        self.assertEqual(windows_record["sender_reconnect"], "windows-post-daemon-restart")

        unix_sender = Sender()
        unix_record: dict[str, Any] = {}
        module.refresh_sender_after_daemon_restart(
            unix_sender,
            unix_record,
            platform_name="posix",
        )
        self.assertEqual(unix_sender.reconnect_calls, 0)
        self.assertNotIn("sender_reconnect", unix_record)

    def test_evidence_names_all_three_rows_and_records_pending_m5_as_host_specific(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="m5", evidence_dir=Path(temporary))
            records = [
                {"id": "daemon-restart-live-receiver", "status": "pass", "delivery_latency_ms": 12.0},
                {"id": "receiver-restart-live-daemon", "status": "pass", "delivery_latency_ms": 8.0},
                {"id": "receiver-crash-within-window", "status": "pass", "delivery_latency_ms": 5.0},
            ]
            json_path, markdown_path = module.write_evidence(args, records)

            self.assertEqual(json_path.name, "restart-matrix-m5.json")
            self.assertEqual(markdown_path.name, "restart-matrix-m5.md")
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("daemon-restart-live-receiver", markdown)
            self.assertIn("receiver-restart-live-daemon", markdown)
            self.assertIn("receiver-crash-within-window", markdown)
            self.assertIn("no remote result is inferred", markdown)

    def test_daemon_launch_uses_plaintext_test_wire_security(self) -> None:
        # The runner's daemon runs from a disposable tempfile home with no
        # provisioned peer HTTPS interface or certificate identity, so it
        # must launch in `plaintext-test` mode, never `mutual-tls` (which
        # requires exactly that provisioning and made the matrix unrunnable
        # on a clean CI host).
        module = load_module()
        argv = module.daemon_launch_argv(Path("/fake/atm-daemon"))
        self.assertEqual(argv, [str(Path("/fake/atm-daemon")), "--peer-wire-security", "plaintext-test"])
        self.assertNotIn("mutual-tls", argv)

    # Numbers from the failing Windows clean-runner record (docs/aq-closeout @
    # 9674f64b7, merge b78c041f1): the product displaced the stale lease at
    # +211 ms, the successor interpreter was `ready` at +933 ms, and delivery
    # landed at +1364 ms -- the old one-tick wall-clock bound failed a row the
    # product had actually passed at bind time.
    WINDOWS_RESTART_AT_NS = 1787951528565134500
    WINDOWS_READY_AT_NS = 1787951529498442200
    WINDOWS_DELIVERED_AT_NS = 1787951529929181800
    WINDOWS_PRE_CRASH_LEASE = {
        "team": "aq1-9-hermes",
        "agent": "aq1-9-receiver",
        "endpoint": "127.0.0.1:51019",
        "registered_at": "2026-08-28T21:11:57.052592Z",
    }
    WINDOWS_SUCCESSOR_LEASE = {
        "team": "aq1-9-hermes",
        "agent": "aq1-9-receiver",
        "endpoint": "127.0.0.1:51039",
        "registered_at": "2026-08-28T21:12:08.776418200Z",
    }

    def classify(self, module, **overrides: Any) -> dict[str, Any]:
        kwargs: dict[str, Any] = {
            "pre_crash_lease": self.WINDOWS_PRE_CRASH_LEASE,
            "leases_after": [self.WINDOWS_SUCCESSOR_LEASE],
            "successor_ready_at_ns": self.WINDOWS_READY_AT_NS,
            "restart_at_ns": self.WINDOWS_RESTART_AT_NS,
            "delivered_at_ns": self.WINDOWS_DELIVERED_AT_NS,
            "delivery_matched": True,
        }
        kwargs.update(overrides)
        return module.classify_crash_recovery(**kwargs)

    def test_no_wall_clock_recovery_bound_remains(self) -> None:
        module = load_module()
        self.assertEqual(module.LEASE_REFRESH_INTERVAL_SECONDS, 1.0)
        self.assertFalse(hasattr(module, "CRASH_RECOVERY_LIMIT_SECONDS"))

    def test_parse_rfc3339_ns_handles_nanosecond_fraction_and_offsets(self) -> None:
        module = load_module()
        base = module.parse_rfc3339_ns("2026-08-28T21:12:08Z")
        self.assertEqual(module.parse_rfc3339_ns("2026-08-28T21:12:08.776418200Z"), base + 776_418_200)
        self.assertEqual(module.parse_rfc3339_ns("2026-08-28T21:12:08.5Z"), base + 500_000_000)
        self.assertEqual(module.parse_rfc3339_ns("2026-08-28T23:12:08+02:00"), base)
        with self.assertRaises(ValueError):
            module.parse_rfc3339_ns("not a timestamp")

    def test_crash_row_passes_on_windows_record_where_wall_clock_bound_tripped(self) -> None:
        module = load_module()
        verdict = self.classify(module)
        self.assertTrue(verdict["displaced_at_bind"], verdict["error"])
        self.assertIsNone(verdict["error"])
        self.assertEqual(verdict["successor_lease_count"], 1)
        self.assertEqual(verdict["pre_crash_endpoint"], "127.0.0.1:51019")
        self.assertEqual(verdict["successor_endpoint"], "127.0.0.1:51039")
        # Diagnostics only: recorded, never asserted by the harness.
        self.assertAlmostEqual(verdict["lease_displaced_at_ms"], 211.284, places=2)
        self.assertAlmostEqual(verdict["successor_spawn_to_ready_ms"], 933.308, places=2)
        self.assertAlmostEqual(verdict["crash_recovery_ms"], 1364.047, places=2)
        self.assertGreater(verdict["crash_recovery_ms"], module.LEASE_REFRESH_INTERVAL_SECONDS * 1000)

    def test_crash_row_fails_when_lease_registered_after_successor_ready(self) -> None:
        # registered_at later than the successor's `ready` event means the
        # stale lease survived bind and was only displaced by a refresh tick.
        module = load_module()
        late = {**self.WINDOWS_SUCCESSOR_LEASE, "registered_at": "2026-08-28T21:12:10.000000000Z"}
        verdict = self.classify(module, leases_after=[late])
        self.assertFalse(verdict["displaced_at_bind"])
        self.assertIn("refresh tick, not at bind", verdict["error"])
        self.assertAlmostEqual(verdict["lease_displaced_at_ms"], 1434.866, places=2)

    def test_crash_row_fails_when_endpoint_unchanged(self) -> None:
        module = load_module()
        stale = {**self.WINDOWS_SUCCESSOR_LEASE, "endpoint": self.WINDOWS_PRE_CRASH_LEASE["endpoint"]}
        verdict = self.classify(module, leases_after=[stale])
        self.assertFalse(verdict["displaced_at_bind"])
        self.assertIn("not a fresh bind", verdict["error"])

    def test_crash_row_fails_when_two_leases_remain(self) -> None:
        module = load_module()
        verdict = self.classify(
            module, leases_after=[self.WINDOWS_PRE_CRASH_LEASE, self.WINDOWS_SUCCESSOR_LEASE]
        )
        self.assertFalse(verdict["displaced_at_bind"])
        self.assertEqual(verdict["successor_lease_count"], 2)
        self.assertIn("exactly one successor lease", verdict["error"])

    def test_crash_row_fails_without_successor_delivery(self) -> None:
        module = load_module()
        verdict = self.classify(module, delivery_matched=False)
        self.assertFalse(verdict["displaced_at_bind"])
        self.assertIn("did not deliver", verdict["error"])

    def test_receiver_leases_filters_doctor_payload_to_receiver_identity(self) -> None:
        module = load_module()
        other = {"team": "aq1-9-hermes", "agent": "aq1-9-sender", "endpoint": "127.0.0.1:1"}
        payload = {"graft_receivers": {"receivers": [other, self.WINDOWS_SUCCESSOR_LEASE]}}
        self.assertEqual(module.receiver_leases(payload), [self.WINDOWS_SUCCESSOR_LEASE])
        self.assertEqual(module.receiver_lease(payload), self.WINDOWS_SUCCESSOR_LEASE)
        self.assertEqual(module.receiver_leases({"graft_receivers": None}), [])
        with self.assertRaises(RuntimeError):
            module.receiver_lease({"graft_receivers": {"receivers": [other]}})

    def test_successor_ready_at_ns_reads_transcript_then_start_result(self) -> None:
        module = load_module()
        transcript = [json.dumps({"kind": "ready", "at_ns": self.WINDOWS_READY_AT_NS}), "not json"]
        self.assertEqual(module.successor_ready_at_ns({"receiver_transcript": transcript}), self.WINDOWS_READY_AT_NS)
        self.assertEqual(
            module.successor_ready_at_ns({"receiver_transcript": [], "receiver_after": {"ready_at_ns": 7}}), 7
        )
        with self.assertRaises(RuntimeError):
            module.successor_ready_at_ns({"receiver_transcript": [], "receiver_after": {"pid": 1}})

    def test_markdown_describes_bind_time_displacement_and_windows_returncode(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-windows", evidence_dir=Path(temporary))
            _, markdown_path = module.write_evidence(args, [])
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("displaced_at_bind", markdown)
            self.assertIn("diagnostics only", markdown)
            self.assertIn("TerminateProcess", markdown)
            self.assertNotIn("one-refresh-tick", markdown)

    def test_scoped_process_environment_restores_prior_values_and_absence(self) -> None:
        module = load_module()
        sentinel_key = "AQ19_RESTART_MATRIX_TEST_PRESENT"
        absent_key = "AQ19_RESTART_MATRIX_TEST_ABSENT"
        os.environ[sentinel_key] = "before"
        os.environ.pop(absent_key, None)
        try:
            with module.scoped_process_environment({sentinel_key: "during", absent_key: "during"}):
                self.assertEqual(os.environ[sentinel_key], "during")
                self.assertEqual(os.environ[absent_key], "during")
            self.assertEqual(os.environ[sentinel_key], "before")
            self.assertNotIn(absent_key, os.environ)
        finally:
            os.environ.pop(sentinel_key, None)
            os.environ.pop(absent_key, None)

    def test_scoped_process_environment_restores_on_exception(self) -> None:
        module = load_module()
        absent_key = "AQ19_RESTART_MATRIX_TEST_EXC_ABSENT"
        os.environ.pop(absent_key, None)
        with self.assertRaises(RuntimeError):
            with module.scoped_process_environment({absent_key: "during"}):
                self.assertEqual(os.environ[absent_key], "during")
                raise RuntimeError("boom")
        self.assertNotIn(absent_key, os.environ)

    # -- stale-evidence clearing (mirrors run_aq4_transfer_evidence.py) --

    def test_clear_stale_evidence_removes_pre_existing_files_and_tolerates_absence(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary)
            json_path = evidence_dir / "restart-matrix-m5.json"
            markdown_path = evidence_dir / "restart-matrix-m5.md"
            json_path.write_text('{"stale": true}', encoding="utf-8")
            markdown_path.write_text("stale", encoding="utf-8")

            # Must not raise even when one of the two paths is already gone.
            module._clear_stale_evidence(json_path, evidence_dir / "does-not-exist.md")

            self.assertFalse(json_path.exists())
            self.assertTrue(markdown_path.exists(), "only the passed-in paths are cleared")

    def test_evidence_output_paths_match_write_evidences_own_naming(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="m5", evidence_dir=Path(temporary))
            json_path, markdown_path = module._evidence_output_paths(args)
            self.assertEqual(json_path.name, "restart-matrix-m5.json")
            self.assertEqual(markdown_path.name, "restart-matrix-m5.md")

    # -- harness_crashed top-level guard around the three run_scenario rows --

    def test_main_never_leaves_a_stale_evidence_file_when_a_row_crashes(self) -> None:
        # Regression coverage for the AQ1.9 sibling of the AQ4 harness
        # fix: a top-level guard in main() around the three run_scenario
        # rows must always (a) delete any pre-existing evidence file for
        # this host before doing any real work, and (b) still write a
        # fresh, non-stale single "harness_crashed" row -- with a
        # traceback -- when one of the three run_scenario calls raises, so
        # a CI `if: always()` artifact-upload step never re-publishes the
        # previous run's committed file as if it were fresh.
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary)
            json_path = evidence_dir / "restart-matrix-m5.json"
            evidence_dir.mkdir(parents=True, exist_ok=True)
            stale_payload = {"schema_version": 1, "sprint": "AQ1.9", "records": [{"status": "pass", "stale": True}]}
            json_path.write_text(json.dumps(stale_payload), encoding="utf-8")

            def _boom(_args: SimpleNamespace, _row: str, _action: str) -> dict[str, Any]:
                raise RuntimeError("owned daemon did not report ready")

            original_run_scenario = module.run_scenario
            original_require_clean_host = module.require_clean_host
            original_argv = sys.argv
            module.run_scenario = _boom
            module.require_clean_host = lambda: None
            sys.argv = [
                "run_hermes_atm_restart_matrix.py",
                "--host",
                "m5",
                "--evidence-dir",
                str(evidence_dir),
                "--daemon",
                str(module.ROOT / "Cargo.toml"),
                "--atm",
                str(module.ROOT / "Cargo.toml"),
            ]
            try:
                exit_code = module.main()
            finally:
                module.run_scenario = original_run_scenario
                module.require_clean_host = original_require_clean_host
                sys.argv = original_argv

            self.assertEqual(exit_code, 1)
            written = json.loads(json_path.read_text(encoding="utf-8"))
            self.assertEqual(len(written["records"]), 1)
            crashed = written["records"][0]
            self.assertEqual(crashed["status"], "harness_crashed")
            self.assertNotIn("stale", crashed)
            self.assertIn("owned daemon did not report ready", crashed["error"])
            self.assertIn("Traceback", crashed["traceback"])
            markdown = (evidence_dir / "restart-matrix-m5.md").read_text(encoding="utf-8")
            self.assertIn("owned daemon did not report ready", markdown)
            self.assertIn("Traceback", markdown)

    def test_main_worker_mode_returns_before_clearing_any_evidence(self) -> None:
        # `--worker` re-invokes this same script as a receiver subprocess
        # (see ReceiverWorker.start); it must never race the parent
        # matrix's own evidence files by clearing them too.
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary)
            json_path = evidence_dir / "restart-matrix-local.json"
            evidence_dir.mkdir(parents=True, exist_ok=True)
            json_path.write_text('{"kept": true}', encoding="utf-8")

            original_worker_main = module.worker_main
            original_argv = sys.argv
            module.worker_main = lambda _args: 0
            sys.argv = [
                "run_hermes_atm_restart_matrix.py",
                "--worker",
                "--workspace-root",
                str(evidence_dir),
                "--evidence-dir",
                str(evidence_dir),
            ]
            try:
                exit_code = module.main()
            finally:
                module.worker_main = original_worker_main
                sys.argv = original_argv

            self.assertEqual(exit_code, 0)
            self.assertTrue(json_path.exists(), "worker mode must never clear the parent's evidence")

    def test_write_evidence_harness_crashed_row_includes_error_and_traceback(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="local", evidence_dir=Path(temporary))
            records = [
                {
                    "id": "harness_crashed",
                    "status": "harness_crashed",
                    "error": "RuntimeError: boom",
                    "traceback": "Traceback (most recent call last):\n  ...\nRuntimeError: boom\n",
                }
            ]
            _json_path, markdown_path = module.write_evidence(args, records)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("HARNESS_CRASHED", markdown)
            self.assertIn("RuntimeError: boom", markdown)
            self.assertIn("Traceback (most recent call last):", markdown)

    def test_write_evidence_surfaces_a_cleanup_warning_without_changing_status(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="local", evidence_dir=Path(temporary))
            records = [
                {
                    "id": "daemon-restart-live-receiver",
                    "status": "pass",
                    "delivery_latency_ms": 3.0,
                    "cleanup_warning": "could not remove /tmp/aq1-9-x after 6 attempts: boom",
                }
            ]
            _json_path, markdown_path = module.write_evidence(args, records)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("PASS", markdown)
            self.assertIn("Cleanup warning", markdown)
            self.assertIn("could not remove /tmp/aq1-9-x", markdown)

    # -- tolerant scratch-directory cleanup (mirrors run_aq4_transfer_evidence.py) --

    def test_remove_tree_tolerant_removes_a_real_directory(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "victim"
            (target / "nested").mkdir(parents=True)
            (target / "nested" / "file.txt").write_text("x", encoding="utf-8")

            result = module._remove_tree_tolerant(target)

            self.assertIsNone(result)
            self.assertFalse(target.exists())

    def test_remove_tree_tolerant_returns_none_for_an_already_missing_path(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary) / "never-created"
            self.assertIsNone(module._remove_tree_tolerant(missing))

    def test_remove_tree_tolerant_reports_a_warning_instead_of_raising_when_removal_never_succeeds(
        self,
    ) -> None:
        # Simulates the observed Windows failure mode (WinError 32/5
        # sharing violation that never clears) without needing an actual
        # locked directory: shutil.rmtree always raises for this path.
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "locked"
            target.mkdir()

            original_rmtree = module.shutil.rmtree

            def _always_fails(_path: object, *_args: object, **_kwargs: object) -> None:
                raise PermissionError("[WinError 32] The process cannot access the file")

            module.shutil.rmtree = _always_fails
            try:
                result = module._remove_tree_tolerant(target, attempts=2, initial_delay=0.0)
            finally:
                module.shutil.rmtree = original_rmtree

            self.assertIsNotNone(result)
            assert result is not None
            self.assertIn("could not remove", result)
            self.assertIn("WinError 32", result)


class OutputCaptureTests(unittest.TestCase):
    """Unit coverage for `OutputCapture.wait_for`'s timeout clamping
    (FTQ-001) and `OutputCapture.join`'s ordering guarantee relative to
    `tail()` (FTQ-002). Uses a real `os.pipe()` in place of a subprocess so
    both are provable deterministically, with no fixed sleeps."""

    def _make_output_capture(self, module: Any) -> tuple[Any, Any, int, int]:
        stdout_read_fd, stdout_write_fd = os.pipe()
        stderr_read_fd, stderr_write_fd = os.pipe()
        process = SimpleNamespace(
            stdout=os.fdopen(stdout_read_fd, "r"),
            stderr=os.fdopen(stderr_read_fd, "r"),
        )
        output = module.OutputCapture(process)
        return output, process, stdout_write_fd, stderr_write_fd

    def test_wait_for_raises_timeout_error_not_value_error_when_clock_jumps_past_deadline(
        self,
    ) -> None:
        # FTQ-001 regression: `wait_for` previously recomputed the
        # remaining time a second time inside
        # `self.events.get(timeout=deadline - time.monotonic())`, after the
        # `while time.monotonic() < deadline` guard had already passed.
        # Under preemption between those two `time.monotonic()` reads, the
        # second read could land past `deadline`, handing
        # `queue.Queue.get` a negative timeout -- which raises
        # `ValueError`, not `queue.Empty` -- crashing the harness instead
        # of reaching the graceful `TimeoutError` path. The fix reads
        # `time.monotonic()` exactly once per loop iteration and clamps
        # the result to zero before it ever reaches `get`, so this second,
        # racing read no longer exists.
        module = load_module()
        output, process, stdout_write_fd, stderr_write_fd = self._make_output_capture(module)
        try:
            # The queue stays empty for the call below: nothing is ever
            # written to either pipe, so both reader threads simply block
            # on read() (daemon threads; harmless at interpreter exit).
            monotonic_readings = iter([100.0, 250.0])
            with patch.object(module.time, "monotonic", side_effect=lambda: next(monotonic_readings)):
                with self.assertRaises(TimeoutError):
                    output.wait_for(lambda _line: True, timeout=5.0)
        finally:
            os.close(stdout_write_fd)
            os.close(stderr_write_fd)
            output.join(timeout=2.0)
            process.stdout.close()
            process.stderr.close()

    def test_wait_for_still_returns_a_line_already_queued(self) -> None:
        # Regression coverage alongside the clamp fix: normal operation (a
        # line already sitting in the queue, no clock jump) is unaffected
        # by the clamp and is still returned promptly.
        module = load_module()
        output, process, stdout_write_fd, stderr_write_fd = self._make_output_capture(module)
        try:
            output.events.put("already queued line")
            line = output.wait_for(lambda candidate: candidate == "already queued line", timeout=5.0)
            self.assertEqual(line, "already queued line")
        finally:
            os.close(stdout_write_fd)
            os.close(stderr_write_fd)
            output.join(timeout=2.0)
            process.stdout.close()
            process.stderr.close()

    def test_join_waits_for_reader_threads_so_tail_reflects_the_final_line(self) -> None:
        # FTQ-002 regression: the reader threads were never joined, so
        # `OwnedDaemon.stop`/`ReceiverWorker.stop` calling `tail()`
        # immediately after `process.wait()` raced the last buffered lines
        # against the reader threads still draining the pipes. `join()`
        # must block until both threads have finished appending everything
        # the pipe delivered before EOF, so `tail()` afterward is complete.
        module = load_module()
        output, process, stdout_write_fd, stderr_write_fd = self._make_output_capture(module)
        with os.fdopen(stdout_write_fd, "w") as writer:
            writer.write("first line\n")
            writer.write("final line written just before close\n")
        os.close(stderr_write_fd)

        output.join(timeout=2.0)

        tail = output.tail()
        self.assertIn("first line", tail)
        self.assertIn("final line written just before close", tail)
        process.stdout.close()
        process.stderr.close()


class FailureCaptureHelperTests(unittest.TestCase):
    """Unit coverage for the small, pure helpers `run_scenario`'s failure path
    is built from: `_cooperative_stop_signal`, `_capture_exception`,
    `_best_effort`, and `_copy_failure_log`. Each is exercised directly, with
    no subprocess or filesystem dependency beyond a scratch `tempfile`
    directory, so the failure-capture behavior is provable without a real
    daemon/atm binary."""

    def test_cooperative_stop_signal_prefers_ctrl_break_only_on_windows_with_support(self) -> None:
        # `stop_process` reads the real `os`/`signal` modules directly (there
        # is no per-call way to inject them into `Popen.terminate`/
        # `send_signal`), so this selection logic is factored out precisely
        # so the platform branch can be driven by explicit booleans instead.
        module = load_module()
        self.assertEqual(module._cooperative_stop_signal(is_windows=True, has_ctrl_break=True), "ctrl_break")
        self.assertEqual(module._cooperative_stop_signal(is_windows=True, has_ctrl_break=False), "terminate")
        self.assertEqual(module._cooperative_stop_signal(is_windows=False, has_ctrl_break=True), "terminate")
        self.assertEqual(module._cooperative_stop_signal(is_windows=False, has_ctrl_break=False), "terminate")

    def test_capture_exception_includes_type_message_and_bounded_traceback_tail(self) -> None:
        module = load_module()
        try:
            raise ValueError("boom")
        except ValueError as error:
            captured = module._capture_exception(error, tail_lines=5)
        self.assertEqual(captured["error"], "ValueError: boom")
        self.assertLessEqual(len(captured["traceback_tail"]), 5)
        self.assertTrue(any("ValueError: boom" in line for line in captured["traceback_tail"]))

    def test_best_effort_returns_result_on_success_and_error_dict_on_exception(self) -> None:
        module = load_module()
        self.assertEqual(module._best_effort(lambda: {"ok": True}), {"ok": True})

        def _boom() -> None:
            raise RuntimeError("nope")

        self.assertEqual(module._best_effort(_boom), {"error": "RuntimeError: nope"})

    def test_copy_failure_log_copies_present_source_and_returns_none_for_missing(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary) / "evidence" / "AQ1.9"
            source = Path(temporary) / "scratch" / "logs" / "atm.log.jsonl"
            source.parent.mkdir(parents=True)
            source.write_text('{"msg": "debug"}\n', encoding="utf-8")

            copied = module._copy_failure_log(source, evidence_dir, "unit-test-row", "unit-test-host")
            self.assertIsNotNone(copied)
            assert copied is not None
            copied_path = Path(copied)
            self.assertEqual(
                copied_path, evidence_dir / "failure-logs" / "unit-test-row-unit-test-host.log.jsonl"
            )
            self.assertEqual(copied_path.read_text(encoding="utf-8"), source.read_text(encoding="utf-8"))

            missing_source = Path(temporary) / "scratch" / "logs" / "does-not-exist.log.jsonl"
            self.assertIsNone(module._copy_failure_log(missing_source, evidence_dir, "row", "host"))


class _FakeOutputCapture:
    """Stands in for `OutputCapture` with a fixed, pre-drained transcript."""

    def __init__(self, lines: list[str]) -> None:
        self._lines = list(lines)

    def tail(self) -> list[str]:
        return list(self._lines)


class _FakeSentMessage:
    def __init__(self, message_id: str) -> None:
        self.message_id = message_id


class _FakeGraftSession:
    """Stands in for `atm_graft.PyGraftSession`: no native extension needed."""

    def __init__(self, caller: Any) -> None:
        self.caller = caller
        self.closed = False

    def send(self, _address: Any, _marker: str, _flag: bool) -> _FakeSentMessage:
        return _FakeSentMessage("fake-message-id")

    def close(self) -> None:
        self.closed = True


class _FakeDaemon:
    """Stands in for `OwnedDaemon`: deterministic output, no real subprocess."""

    def __init__(self, _binary: Path, _env: dict[str, str], _timeout: float) -> None:
        self.output = _FakeOutputCapture(["ATM_DAEMON_READY"])

    def start(self) -> dict[str, Any]:
        self.output = _FakeOutputCapture(["ATM_DAEMON_READY"])
        return {"pid": 4242, "output_tail": self.output.tail()}

    def stop(self) -> dict[str, Any]:
        return {"pid": 4242, "returncode": 0, "output_tail": self.output.tail()}


class _FakeReceiver:
    """Stands in for `ReceiverWorker`: deterministic output, no real subprocess."""

    def __init__(self, _script: Path, _workspace_root: Path, _env: dict[str, str], _timeout: float) -> None:
        self.output = _FakeOutputCapture([json.dumps({"kind": "ready", "at_ns": 1})])

    def start(self) -> dict[str, Any]:
        self.output = _FakeOutputCapture([json.dumps({"kind": "ready", "at_ns": 1})])
        return {"pid": 4343, "ready_at_ns": 1}

    def wait_for_nudge(self, marker: str, _timeout: float) -> dict[str, Any]:
        return {"at_ns": 2, "body": marker}

    def stop(self, *, crash: bool = False, cooperative: bool = False) -> dict[str, Any]:
        return {"pid": 4343, "returncode": 0, "crash": crash, "output_tail": self.output.tail()}


class RunScenarioFailureCaptureTests(unittest.TestCase):
    """End-to-end coverage of `run_scenario`'s failure-capture wiring, using
    fakes for `OwnedDaemon`/`ReceiverWorker`/`atm_graft` (no real daemon or
    `atm` binary, no subprocess, no sleeps) so both the failure path
    (diagnostics captured, log copied) and the success path (no
    `failure-logs` file at all) are provable deterministically."""

    def test_run_scenario_failure_path_captures_error_transcripts_and_copies_log(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary) / "evidence"
            args = SimpleNamespace(
                host="unit-test-host",
                evidence_dir=evidence_dir,
                atm=Path("/nonexistent/atm"),
                daemon=Path("/nonexistent/atm-daemon"),
                timeout=1.0,
            )

            def fake_add_roster_member(
                _atm: Path, env: dict[str, str], _home: Path, _member: str
            ) -> None:
                # A real daemon would already have written ATM_LOG=debug
                # output by the time a row fails; simulate that so the
                # failure-log copy below has a source file to find.
                log_dir = Path(env["ATM_LOG_DIR"])
                log_dir.mkdir(parents=True, exist_ok=True)
                (log_dir / "atm.log.jsonl").write_text('{"msg": "debug line"}\n', encoding="utf-8")
                raise RuntimeError("simulated setup failure before daemon start")

            original_add_roster_member = module.add_roster_member
            original_doctor = module.doctor
            module.add_roster_member = fake_add_roster_member
            module.doctor = lambda _atm, _env: {"ok": True}
            try:
                record = module.run_scenario(args, "unit-test-row", "daemon_restart")
            finally:
                module.add_roster_member = original_add_roster_member
                module.doctor = original_doctor

            self.assertEqual(record["status"], "fail")
            self.assertIn("RuntimeError", record["error"])
            self.assertIn("simulated setup failure", record["error"])
            self.assertTrue(record["traceback_tail"])
            self.assertIn("Traceback", "\n".join(record["traceback_tail"]))
            # Neither daemon nor receiver ever started before the row failed.
            self.assertEqual(record["daemon_transcript"], {"before": [], "after": []})
            self.assertEqual(record["receiver_transcript"], {"before": [], "after": []})
            self.assertEqual(record["doctor_after"], {"ok": True})

            failure_log_path = record["failure_log_path"]
            self.assertIsNotNone(failure_log_path)
            assert failure_log_path is not None
            copied = Path(failure_log_path)
            self.assertTrue(copied.exists())
            self.assertEqual(copied.name, "unit-test-row-unit-test-host.log.jsonl")
            self.assertEqual(copied.parent.name, "failure-logs")
            self.assertIn("debug line", copied.read_text(encoding="utf-8"))

    def test_run_scenario_success_leaves_no_failure_log_file(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary) / "evidence"
            args = SimpleNamespace(
                host="unit-test-host",
                evidence_dir=evidence_dir,
                atm=Path("/nonexistent/atm"),
                daemon=Path("/nonexistent/atm-daemon"),
                timeout=1.0,
            )

            fake_atm_graft = SimpleNamespace(
                PyAgentAddress=lambda agent, team, extra=None: (agent, team, extra),
                PyGraftSession=_FakeGraftSession,
            )
            patches = {
                "OwnedDaemon": _FakeDaemon,
                "ReceiverWorker": _FakeReceiver,
                "doctor": lambda _atm, _env: {"graft_receivers": {"receivers": []}},
                "add_roster_member": lambda *_a, **_k: None,
                "remove_roster_member": lambda *_a, **_k: None,
            }
            originals = {name: getattr(module, name) for name in patches}
            for name, value in patches.items():
                setattr(module, name, value)
            try:
                with patch.dict(sys.modules, {"atm_graft": fake_atm_graft}):
                    record = module.run_scenario(args, "unit-test-row", "daemon_restart")
            finally:
                for name, value in originals.items():
                    setattr(module, name, value)

            self.assertEqual(record["status"], "pass")
            self.assertNotIn("error", record)
            failure_logs_dir = evidence_dir / "failure-logs"
            self.assertFalse(failure_logs_dir.exists(), "a passing row must not write any failure-logs file")


if __name__ == "__main__":
    unittest.main()

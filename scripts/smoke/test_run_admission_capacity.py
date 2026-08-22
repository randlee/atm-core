"""Focused unit tests for the AI.33 public admission-capacity runner."""
from __future__ import annotations

import argparse
import contextlib
import importlib.util
import inspect
import io
import json
import os
from contextlib import ExitStack, closing
from pathlib import Path, PureWindowsPath
import plistlib
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

from scripts.smoke.benchmark_schema import compact_evidence, distribution, percentile
from scripts.smoke import benchmark_snapshot as SNAPSHOT


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


def complete_evidence(**overrides):
    evidence = {
        "schema_version": 2,
        "generated_at": "2026-08-01T05:00:00.123456Z",
        "host_label": "test-host",
        "transport": "tcp",
        "frames_per_connection": 1,
        "messages_per_connection": 1,
        "requested_messages_per_sample": 1_000,
        "minimum_sample_count": 1,
        "sample_count": 1,
        "target_duration_s": 1.0,
        "run_duration_s": 1.0,
        "runs": [{"intervals": [{
            "accepted_count": 1_000, "requested_count": 1_000,
            "response_count": 1_000, "elapsed_seconds": 1.0,
            "admissions_per_second": 1_000.0, "connections": 1_000,
            "connections_per_second": 1_000.0, "request_frames_per_second": 1_000.0,
            "application_wire_bytes": {"request": 1_000, "response": 1_000, "total": 2_000},
            "application_wire_bytes_per_second": 2_000.0, "time_to_send_1k_s": 1.0,
            "latency_ms": {"min": 0.5, "p50": 0.5, "p95": 0.5, "max": 0.5},
            "passed": True, "first_failure": None,
        }]}],
        "passed": True,
    }
    evidence.update(overrides)
    return evidence


def healthy_managed_status() -> dict[str, object]:
    return {
        "atm": {
            "selector": "/active/atm",
            "target": "/release/atm",
            "version": "atm 1.4.1-beta-ai-1",
        },
        "atm_daemon": {"selector": "/active/atm-daemon", "target": "/release/atm-daemon"},
        "live_pair": {"matched": True, "detail": "selected executable is live"},
        "doctor": {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
            "daemon_context": {"version": "1.4.1-beta-ai-1"},
        },
    }


class AdmissionCapacityTests(unittest.TestCase):
    def test_ordinary_benchmark_runs_all_four_f8_targets_in_fixed_order(self):
        captured: list[dict[str, object]] = []

        def run_capacity(*_args, **kwargs):
            captured.append(kwargs)
            return RUNNER.CapacityRunResult(0, Path("sentinel.evidence"), Path("sentinel.raw"))

        measured = mock.Mock(
            median_msg_per_second=45_000.0,
            p95_msg_per_second=45_100.0,
            p99_msg_per_second=45_200.0,
            accepted=10_000,
            requested=10_000,
            raw_artifact="artifacts/raw.json",
        )

        with (
            mock.patch.object(sys, "argv", ["run_admission_capacity.py"]),
            mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity),
            mock.patch.object(RUNNER, "suite_target_result", return_value=measured),
            contextlib.redirect_stdout(stdout := io.StringIO()),
        ):
            self.assertEqual(RUNNER.main(), 0)

        self.assertEqual(
            [item["benchmark_target"] for item in captured],
            ["sqlite", "uds", "tcp", "tcp-tls"],
        )
        self.assertTrue(all(item["comparison_required"] is False for item in captured))
        self.assertTrue(all(item["raw_evidence_directory"] == RUNNER.DEFAULT_RAW_EVIDENCE_DIR for item in captured))
        self.assertEqual(stdout.getvalue().count("p50=45000.00 msg/s"), 4)

    def test_selected_profile_without_diagnostic_marker_is_rejected(self):
        with mock.patch.object(sys, "argv", ["run_admission_capacity.py", "--target", "tcp"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "require --diagnostic-only"):
                RUNNER.main()

    def test_direct_production_writer_profile_keeps_real_interval_counts(self):
        stdout = json.dumps(
            {
                "kind": "canonical_core_write",
                "requested_count": 2_000,
                "accepted_count": 2_000,
                "worker_count": 64,
                "elapsed_seconds": 1.0,
                "admissions_per_second": 2_000.0,
                "intervals": [
                    {
                        "requested_count": 1_000, "accepted_count": 1_000,
                        "elapsed_seconds": 0.4, "admissions_per_second": 2_500.0,
                    },
                    {
                        "requested_count": 1_000, "accepted_count": 1_000,
                        "elapsed_seconds": 0.6, "admissions_per_second": 1666.0,
                    },
                ],
            }
        )
        roster = RUNNER.CapacityRoster.unique()
        with mock.patch.object(
            RUNNER.subprocess, "run", return_value=mock.Mock(returncode=0, stdout=stdout, stderr=""),
        ) as run:
            profile, direct = RUNNER.run_direct_production_writer_profile(
                Path("/bin/benchmark"), {"ATM_HOME": "/tmp/atm-capacity-test"}, roster,
                1_000, 2, 64,
            )

        self.assertEqual(profile["sample_count"], 2)
        self.assertEqual(profile["intervals"][0]["accepted_count"], 1_000)
        self.assertEqual(direct["kind"], "canonical_core_write")
        self.assertIn("--seconds", run.call_args.args[0])

    def test_sqlite_probe_build_is_separate_from_the_released_daemon_pair(self):
        probe = RUNNER.ROOT / "target" / "release" / "atm-daemon-benchmark"
        completed = subprocess.CompletedProcess(["cargo"], 0, "", "")
        with (
            mock.patch.object(RUNNER.Path, "is_file", side_effect=[False, True]),
            mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as command,
        ):
            self.assertEqual(RUNNER.canonical_writer_probe(), probe)
        command.assert_called_once()
        self.assertIn("atm-daemon-bootstrap", command.call_args.args[0])
        self.assertIn("benchmark-harness", command.call_args.args[0])

    def test_capacity_run_result_preserves_compact_and_raw_evidence(self):
        result = RUNNER.CapacityRunResult(0, Path("compact.json"), Path("raw.json"))
        code, compact = result
        self.assertEqual((code, compact, result.raw_evidence_path), (0, Path("compact.json"), Path("raw.json")))

    def test_suite_target_result_retains_measured_below_floor_evidence(self):
        """A nonzero suite target can be a useful measured remediation result."""
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "artifacts"
            artifacts.mkdir()
            evidence = complete_evidence(
                transport="tcp",
                peer_wire_security="plaintext-test",
                benchmark_target="tcp",
            )
            compact = artifacts / "compact.json"
            raw = artifacts / "raw.json"
            compact.write_text(compact_evidence(evidence).model_dump_json(), encoding="utf-8")
            raw.write_text(json.dumps(evidence), encoding="utf-8")

            target = RUNNER.suite_target_result(
                "tcp", RUNNER.CapacityRunResult(1, compact, raw), artifact_root=artifacts,
            )

        self.assertEqual(target.target, "tcp")
        self.assertEqual(target.requested, 1_000)
        self.assertEqual(target.accepted, 1_000)
        self.assertEqual(target.errors, 0)
        self.assertEqual(target.raw_artifact, "raw.json")

    def test_suite_target_result_rejects_mismatched_or_unmeasured_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "artifacts"
            artifacts.mkdir()
            raw = artifacts / "raw.json"
            raw.write_text("{}", encoding="utf-8")
            mismatch = artifacts / "mismatch.json"
            mismatch.write_text(
                compact_evidence(complete_evidence(
                    transport="tcp", peer_wire_security="mutual-tls", benchmark_target="tcp-tls",
                )).model_dump_json(),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.SmokeError, "does not match"):
                RUNNER.suite_target_result(
                    "tcp", RUNNER.CapacityRunResult(0, mismatch, raw), artifact_root=artifacts,
                )

            unmeasured = artifacts / "unmeasured.json"
            unmeasured.write_text(
                compact_evidence(complete_evidence(
                    transport="tcp", peer_wire_security="plaintext-test", benchmark_target="tcp",
                    runs=[], passed=False, failure="setup failed",
                )).model_dump_json(),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RUNNER.SmokeError, "did not reach a measured interval"):
                RUNNER.suite_target_result(
                    "tcp", RUNNER.CapacityRunResult(1, unmeasured, raw), artifact_root=artifacts,
                )

    def _run_snapshot_lifecycle_case(self, fault: str | None = None) -> tuple[int, dict[str, object], dict[str, mock.Mock]]:
        """Exercise the runner lifecycle without inspecting a primary-account root."""
        captured: dict[str, object] = {}
        calls: dict[str, mock.Mock] = {}
        profile = {
            "sample_count": 1,
            "target_duration_s": 1.0,
            "run_duration_s": 1.0,
            "intervals": complete_evidence()["runs"][0]["intervals"],
        }
        snapshot = mock.Mock(
            snapshot_id="snapshot-20260822T000000Z-0123456789abcdef",
            account_id="uid:4242",
            user_version=1,
            page_count=2,
            byte_count=8192,
            sha256="1" * 64,
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interactive_database = root / "interactive" / ".atm" / "db" / "mail.db"
            interactive_database.parent.mkdir(parents=True)
            interactive_database.write_bytes(b"interactive account must remain unchanged")
            original_interactive = interactive_database.read_bytes()
            home = root / "atm-capacity-case"
            atm = root / "atm"
            daemon = root / "atm-daemon"
            atm.touch()
            daemon.touch()
            account = mock.Mock(account_id="uid:4242", durable_state_root=root / "benchmark" / ".atm" / "db")
            process = mock.Mock(pid=123)
            output = mock.Mock()
            output.evidence.return_value = {}

            with ExitStack() as stack:
                calls["release"] = stack.enter_context(
                    mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]),
                )
                stack.enter_context(
                    mock.patch.object(RUNNER, "require_capacity_benchmark_account", return_value=account),
                )
                stack.enter_context(mock.patch.object(RUNNER, "require_clean_host_daemon_state"))
                stack.enter_context(mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]))
                stack.enter_context(mock.patch.object(RUNNER, "runtime_environment", return_value={}))
                stack.enter_context(mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64))
                calls["start"] = stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "start_capacity_daemon",
                        side_effect=(RUNNER.SmokeError("injected snapshot start failure") if fault == "start" else None),
                        return_value=(process, output),
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "command_result", return_value={}))
                stack.enter_context(mock.patch.object(RUNNER, "benchmark_doctor_payload", return_value={}))
                stack.enter_context(mock.patch.object(RUNNER, "prepare_capacity_roster"))
                stack.enter_context(
                    mock.patch.object(RUNNER, "local_endpoint", return_value=RUNNER.LocalEndpoint("tcp", "127.0.0.1:1")),
                )
                stack.enter_context(mock.patch.object(RUNNER, "run_cached_roster_heartbeat_probe", return_value={}))
                calls["profile"] = stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "run_profile",
                        side_effect=(RUNNER.SmokeError("injected profile failure") if fault == "profile" else None),
                        return_value=profile,
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "release_version", return_value="atm test"))
                stack.enter_context(mock.patch.object(RUNNER, "load_baseline_median", return_value=None))
                stack.enter_context(mock.patch.object(RUNNER, "baseline_reference", return_value=None))
                stack.enter_context(
                    mock.patch.object(RUNNER, "evaluate_profile_thresholds", return_value={"passed": True}),
                )
                calls["snapshot"] = stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "create_verified_snapshot",
                        side_effect=(RUNNER.BenchmarkSnapshotError("injected snapshot failure") if fault == "snapshot" else None),
                        return_value=snapshot,
                    ),
                )
                calls["restore"] = stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "restore_verified_snapshot",
                        side_effect=(RUNNER.BenchmarkSnapshotError("injected restore failure") if fault == "restore" else None),
                        return_value=snapshot,
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "verify_active_snapshot", return_value=snapshot))
                calls["reap"] = stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "reap_owned_daemon",
                        side_effect=(subprocess.TimeoutExpired("atm-daemon", 10) if fault == "stop" else None),
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "write_raw_evidence", return_value=root / "raw.json"))
                stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "write_evidence",
                        side_effect=lambda _path, value: (captured.update(value), root / "evidence.json")[1],
                    ),
                )
                code, _evidence_path = RUNNER.run_capacity(
                    home, root, "tcp", 1, sample_count=1, raw_evidence_directory=root,
                )

            self.assertEqual(interactive_database.read_bytes(), original_interactive)
        return code, captured, calls

    def test_snapshot_lifecycle_restores_clean_baseline_after_success(self):
        code, captured, calls = self._run_snapshot_lifecycle_case()

        self.assertEqual(code, 0)
        self.assertEqual(calls["snapshot"].call_count, 1)
        self.assertEqual(calls["restore"].call_count, 1)
        self.assertEqual(captured["clean_baseline_snapshot"]["snapshot_id"], "snapshot-20260822T000000Z-0123456789abcdef")
        self.assertEqual(captured["post_restore_snapshot"]["snapshot_id"], "snapshot-20260822T000000Z-0123456789abcdef")
        self.assertTrue(all(item["duration_s"] >= 0 for entries in captured["lifecycle"].values() for item in entries))
        self.assertEqual(calls["start"].call_count, 2)

    def test_real_snapshot_lifecycle_never_touches_interactive_root(self):
        """Exercise the real account-bound snapshot APIs through ``run_capacity``.

        The daemon, roster, and public profile are deliberately stubbed: this
        test proves the filesystem boundary, not the network benchmark.  The
        snapshot module itself is not mocked, so create, restore, and verify
        operate on a real disposable SQLite database.
        """
        profile = {
            "sample_count": 1,
            "target_duration_s": 1.0,
            "run_duration_s": 1.0,
            "intervals": complete_evidence()["runs"][0]["intervals"],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            interactive_root = root / "interactive" / ".atm"
            interactive_database = interactive_root / "db" / "mail.db"
            interactive_database.parent.mkdir(parents=True)
            interactive_database.write_bytes(b"interactive account must remain unchanged")
            (interactive_root / "identity.json").write_text('{"interactive":true}\n', encoding="utf-8")
            interactive_before = {
                path.relative_to(interactive_root): path.read_bytes()
                for path in interactive_root.rglob("*")
                if path.is_file()
            }

            benchmark_home = root / "benchmark"
            benchmark_state = benchmark_home / ".atm" / "db"
            benchmark_state.mkdir(parents=True)
            benchmark_database = benchmark_state / SNAPSHOT.MAIL_DATABASE_NAME
            with closing(sqlite3.connect(benchmark_database)) as connection:
                connection.execute("CREATE TABLE benchmark_rows(value INTEGER NOT NULL)")
                connection.execute("INSERT INTO benchmark_rows(value) VALUES (1)")
                connection.execute("PRAGMA user_version = 54")
                connection.commit()
            account = RUNNER.BenchmarkAccount(
                account_id="uid:benchmark-test",
                home=benchmark_home,
                durable_state_root=benchmark_state,
                manifest_path=benchmark_home / ".atm" / "benchmark-account.json",
            )
            home = root / "atm-capacity-real-snapshot"
            atm = root / "atm"
            daemon = root / "atm-daemon"
            atm.touch()
            daemon.touch()
            process = mock.Mock(pid=123)
            output = mock.Mock()
            output.evidence.return_value = {}
            captured: dict[str, object] = {}

            with ExitStack() as stack:
                stack.enter_context(mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]))
                stack.enter_context(
                    mock.patch.object(RUNNER, "require_capacity_benchmark_account", return_value=account),
                )
                # The real snapshot functions resolve their account through
                # this account-local seam; no snapshot API is replaced.
                stack.enter_context(
                    mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account),
                )
                stack.enter_context(mock.patch.object(RUNNER, "require_clean_host_daemon_state"))
                stack.enter_context(mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]))
                stack.enter_context(mock.patch.object(RUNNER, "runtime_environment", return_value={}))
                stack.enter_context(mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64))
                stack.enter_context(
                    mock.patch.object(RUNNER, "start_capacity_daemon", return_value=(process, output)),
                )
                stack.enter_context(mock.patch.object(RUNNER, "reap_owned_daemon"))
                stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "command_result",
                        return_value={
                            "exit_code": 0,
                            "stderr": "",
                            "stdout": json.dumps({
                                "summary": {"status": "healthy"},
                                "runtime_status": {"liveness": "running", "readiness": "ready"},
                            }),
                        },
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "prepare_capacity_roster"))
                stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "local_endpoint",
                        return_value=RUNNER.LocalEndpoint("tcp", ("127.0.0.1", 1)),
                    ),
                )
                stack.enter_context(mock.patch.object(RUNNER, "run_cached_roster_heartbeat_probe", return_value={}))
                stack.enter_context(mock.patch.object(RUNNER, "run_profile", return_value=profile))
                stack.enter_context(mock.patch.object(RUNNER, "release_version", return_value="atm test"))
                stack.enter_context(mock.patch.object(RUNNER, "load_baseline_median", return_value=None))
                stack.enter_context(mock.patch.object(RUNNER, "baseline_reference", return_value=None))
                stack.enter_context(
                    mock.patch.object(RUNNER, "evaluate_profile_thresholds", return_value={"passed": True}),
                )
                stack.enter_context(mock.patch.object(RUNNER, "write_raw_evidence", return_value=root / "raw.json"))
                stack.enter_context(
                    mock.patch.object(
                        RUNNER,
                        "write_evidence",
                        side_effect=lambda _path, evidence: (captured.update(evidence), root / "evidence.json")[1],
                    ),
                )
                code, _evidence_path = RUNNER.run_capacity(
                    home, root, "tcp", 1, sample_count=1, raw_evidence_directory=root,
                )

            self.assertEqual(code, 0)
            self.assertEqual(
                {
                    path.relative_to(interactive_root): path.read_bytes()
                    for path in interactive_root.rglob("*")
                    if path.is_file()
                },
                interactive_before,
            )
            self.assertFalse((interactive_root / SNAPSHOT.SNAPSHOT_ROOT_NAME).exists())
            self.assertIn("clean_baseline_snapshot", captured)
            self.assertIn("restored_clean_baseline", captured)
            self.assertIn("post_restore_snapshot", captured)
            self.assertEqual(
                captured["clean_baseline_snapshot"]["snapshot_id"],
                captured["post_restore_snapshot"]["snapshot_id"],
            )

    def test_runner_reaches_only_account_bound_snapshot_apis_not_primary_state_mutation(self):
        source = inspect.getsource(RUNNER.run_capacity)

        self.assertIn("create_verified_snapshot", source)
        self.assertIn("restore_verified_snapshot", source)
        self.assertNotIn("HostStateBackup", source)
        self.assertNotIn("ManagedDaemonLifecycle", source)
        self.assertNotIn("os.replace(", source)
        self.assertNotIn("os.rename(", source)

    def test_snapshot_failure_prevents_roster_and_restore(self):
        code, captured, calls = self._run_snapshot_lifecycle_case("snapshot")

        self.assertEqual(code, 1)
        self.assertEqual(calls["profile"].call_count, 0)
        self.assertEqual(calls["restore"].call_count, 0)
        self.assertEqual(captured["lifecycle"]["snapshot"][-1]["status"], "failed")

    def test_profile_failure_still_restores_the_published_baseline(self):
        code, captured, calls = self._run_snapshot_lifecycle_case("profile")

        self.assertEqual(code, 1)
        self.assertEqual(calls["snapshot"].call_count, 1)
        self.assertEqual(calls["restore"].call_count, 1)
        self.assertEqual(captured["lifecycle"]["profile"][-1]["status"], "failed")

    def test_stop_failure_never_attempts_snapshot_restore(self):
        code, captured, calls = self._run_snapshot_lifecycle_case("stop")

        self.assertEqual(code, 1)
        self.assertEqual(calls["snapshot"].call_count, 0)
        self.assertEqual(calls["restore"].call_count, 0)
        self.assertEqual(captured["lifecycle"]["stop"][-1]["status"], "failed")

    def test_restore_failure_is_retained_as_a_stable_lifecycle_failure(self):
        code, captured, calls = self._run_snapshot_lifecycle_case("restore")

        self.assertEqual(code, 1)
        self.assertEqual(calls["restore"].call_count, 1)
        self.assertEqual(captured["lifecycle"]["restore"][-1]["status"], "failed")
        self.assertIn("recovery:", captured["failure"])

    def test_host_runtime_doctor_environment_ignores_disposable_atm_home(self):
        environment = {
            "ATM_HOME": "/tmp/atm-capacity-1",
            "ATM_IDENTITY": "capacity-agent",
            "ATM_TEAM": "capacity-team",
        }
        self.assertEqual(
            RUNNER.host_runtime_client_environment(environment),
            {"ATM_IDENTITY": "capacity-agent", "ATM_TEAM": "capacity-team"},
        )
        self.assertIn("ATM_HOME", environment)

    def test_benchmark_doctor_rejects_a_ready_runtime_with_unhealthy_observability(self):
        result = {
            "exit_code": 1,
            "stderr": "",
            "stdout": json.dumps({
                "summary": {"status": "error"},
                "findings": [{"code": "ATM_OBSERVABILITY_HEALTH_FAILED"}],
                "runtime_status": {"liveness": "running", "readiness": "ready"},
            }),
        }
        with self.assertRaisesRegex(RUNNER.SmokeError, "capacity doctor"):
            RUNNER.benchmark_doctor_payload(result)

    def test_benchmark_doctor_rejects_other_or_not_ready_failure(self):
        for payload in (
            {
                "summary": {"status": "error"},
                "findings": [{"code": "ATM_MAIL_STORE_FAILED"}],
                "runtime_status": {"liveness": "running", "readiness": "ready"},
            },
            {
                "summary": {"status": "error"},
                "findings": [{"code": "ATM_OBSERVABILITY_HEALTH_FAILED"}],
                "runtime_status": {"liveness": "running", "readiness": "draining"},
            },
        ):
            with self.subTest(payload=payload):
                with self.assertRaisesRegex(RUNNER.SmokeError, "capacity doctor"):
                    RUNNER.benchmark_doctor_payload(
                        {"exit_code": 1, "stderr": "", "stdout": json.dumps(payload)},
                    )

    def test_compaction_math_matches_hand_calculated_intervals(self):
        values = [10.0, 20.0, 30.0, 40.0]
        self.assertEqual(percentile(values, 0.95), 40.0)
        self.assertEqual(
            distribution(values),
            {"min": 10.0, "p50": 25.0, "p95": 40.0, "p99": 40.0, "max": 40.0},
        )

        base_interval = complete_evidence()["runs"][0]["intervals"][0]
        intervals = [
            {
                **base_interval,
                "accepted_count": index,
                "requested_count": index,
                "response_count": index,
                "admissions_per_second": value,
                "connections": index,
                "connections_per_second": value / 2,
                "request_frames_per_second": value,
                "application_wire_bytes": {"request": index * 10, "response": index * 20, "total": index * 30},
                "application_wire_bytes_per_second": value * 30,
                "time_to_send_1k_s": 1_000 / value,
                "latency_ms": {"min": index, "p50": index * 2, "p95": index * 3, "max": index * 4},
            }
            for index, value in enumerate(values, start=1)
        ]
        summary = compact_evidence(
            complete_evidence(
                minimum_sample_count=4,
                sample_count=4,
                target_duration_s=4.0,
                run_duration_s=4.0,
                runs=[{"intervals": intervals}],
            )
        )

        self.assertEqual(summary.metrics.interval_count, 4)
        self.assertEqual(summary.metrics.accepted_count, 10)
        self.assertEqual(summary.metrics.application_wire_bytes.total, 300)
        self.assertEqual(
            summary.metrics.admissions_per_second.model_dump(),
            {"min": 10.0, "p50": 25.0, "p95": 40.0, "p99": 40.0, "max": 40.0},
        )
        self.assertEqual(
            summary.metrics.interval_latency_ms.model_dump(),
            {"min": 1.0, "p50": 5.0, "p95": 12.0, "p99": 12.0, "max": 16.0},
        )

    def test_home_rejects_production_or_non_temporary_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            production_home = Path(temp) / "capacity-user"
            with mock.patch.object(RUNNER, "os_account_home", return_value=production_home):
                with self.assertRaisesRegex(RUNNER.SmokeError, "production"):
                    RUNNER.validate_capacity_home(production_home / ".atm")
        with self.assertRaisesRegex(RUNNER.SmokeError, "basename"):
            RUNNER.validate_capacity_home(Path(tempfile.gettempdir()) / "shared-atm")

    def test_home_accepts_only_a_marked_temporary_directory(self):
        path = Path(tempfile.gettempdir()) / "atm-capacity-unit-home"
        self.assertEqual(RUNNER.validate_capacity_home(path), path.resolve())

    def test_isolation_requires_a_valid_manifest_not_environment_assertion(self):
        with mock.patch.dict(os.environ, {"ATM_CAPACITY_ISOLATED_OS_USER": "1"}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "environment assertion"):
                RUNNER.require_capacity_benchmark_account()
        with mock.patch.dict(os.environ, {"ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE": "1"}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "is retired"):
                RUNNER.require_capacity_benchmark_account()
        account = mock.Mock(durable_state_root=Path("/benchmark/.atm/db"))
        with mock.patch.object(RUNNER, "require_benchmark_account", return_value=account):
            self.assertEqual(RUNNER.require_capacity_benchmark_account(), account)

    def test_host_state_backup_refuses_without_changing_the_primary_database(self):
        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            original = os_home / ".atm" / "db"
            original.mkdir(parents=True)
            (original / "mail.db").write_text("prior state", encoding="utf-8")
            with mock.patch.object(RUNNER, "os_account_home", return_value=os_home):
                with self.assertRaisesRegex(RUNNER.SmokeError, "refusing to replace"):
                    RUNNER.HostStateBackup.begin()
            self.assertEqual((original / "mail.db").read_text(encoding="utf-8"), "prior state")

    def test_launch_agent_override_preserves_source_and_replaces_only_wire_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "com.example.atm.plist"
            original_payload = {
                "Label": "com.example.atm",
                "ProgramArguments": [
                    "/usr/bin/env", "-u", "ATM_TEAM", "/selected/atm-daemon",
                    "--peer-wire-security", "mutual-tls", "--other-option", "value",
                ],
            }
            with source.open("wb") as handle:
                plistlib.dump(original_payload, handle)
            source_bytes = source.read_bytes()

            override = RUNNER.LaunchAgentPeerWireOverride.create(source, "plaintext-test")
            with override.override_path.open("rb") as handle:
                override_payload = plistlib.load(handle)

            self.assertEqual(source.read_bytes(), source_bytes)
            self.assertEqual(
                override_payload["ProgramArguments"],
                [
                    "/usr/bin/env", "-u", "ATM_TEAM", "/selected/atm-daemon",
                    "--other-option", "value", "--peer-wire-security", "plaintext-test",
                ],
            )
            override.assert_source_unchanged()
            override.cleanup()
            self.assertFalse(override.override_path.exists())

    def test_launch_agent_override_can_change_only_its_temporary_log_level(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "com.example.atm.plist"
            payload = {
                "Label": "com.example.atm",
                "ProgramArguments": ["/selected/atm-daemon"],
                "EnvironmentVariables": {"ATM_LOG": "debug", "HOME": "/example"},
            }
            with source.open("wb") as handle:
                plistlib.dump(payload, handle)
            source_bytes = source.read_bytes()

            override = RUNNER.LaunchAgentPeerWireOverride.create(
                source, "plaintext-test", "off",
            )
            with override.override_path.open("rb") as handle:
                override_payload = plistlib.load(handle)

            self.assertEqual(source.read_bytes(), source_bytes)
            self.assertEqual(
                override_payload["EnvironmentVariables"], {"ATM_LOG": "off", "HOME": "/example"},
            )
            override.cleanup()

    def test_managed_mode_requires_the_explicit_launch_agent_plist(self):
        lifecycle = RUNNER.ManagedDaemonLifecycle(
            RUNNER.ManagedDaemonOptions(service="com.example.atm"),
            peer_wire_security="plaintext-test",
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "managed-launch-agent-plist"):
            lifecycle.isolated_options()

    def test_managed_mode_derives_selector_links_from_pre_quiesce_status(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        resolved = RUNNER.resolved_managed_selector_links(options, healthy_managed_status())

        self.assertEqual(resolved.cli_link, Path("/active/atm"))
        self.assertEqual(resolved.daemon_link, Path("/active/atm-daemon"))

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_managed_mode_uses_temporary_plist_then_restarts_original_pair(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "com.example.atm.plist"
            source_payload = {
                "Label": "com.example.atm",
                "ProgramArguments": ["/selected/atm-daemon"],
            }
            with source.open("wb") as handle:
                plistlib.dump(source_payload, handle)
            source_bytes = source.read_bytes()
            options = RUNNER.ManagedDaemonOptions(
                service="com.example.atm", launch_agent_plist=source,
            )
            status = healthy_managed_status()
            status["doctor"]["daemon_context"]["peer_wire_security"] = "plaintext-test"
            calls: list[tuple[str, Path, bool]] = []

            def daemon_switch(action, passed_options, *, doctor=False):
                assert passed_options.launch_agent_plist is not None
                calls.append((action, passed_options.launch_agent_plist, doctor))
                return status

            os_home = root / "os-home"
            state = os_home / ".atm"
            state.mkdir(parents=True)
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
            ):
                lifecycle = RUNNER.ManagedDaemonLifecycle(
                    options, peer_wire_security="plaintext-test",
                )
                lifecycle.begin()
                lifecycle.start_isolated_service()
                override_path = lifecycle.launch_override.override_path
                lifecycle.restart_isolated_service()
                lifecycle.restore()

            self.assertEqual(source.read_bytes(), source_bytes)
            self.assertEqual((state / "mail.db").read_text(encoding="utf-8"), "managed-state")
            self.assertFalse(override_path.exists())
            self.assertEqual(
                [(action, path == source, doctor) for action, path, doctor in calls],
                [
                    ("status", True, False), ("quiesce", True, False),
                    ("restart", False, False), ("status", False, True),
                    ("quiesce", False, False), ("restart", False, False), ("status", False, True),
                    ("quiesce", False, False), ("restart", True, False), ("status", True, True),
                ],
            )

    def test_undeclared_host_state_refuses_before_backup_or_daemon_quiesce(self):
        with (
            mock.patch.dict(
                os.environ,
                {"ATM_CAPACITY_ISOLATED_OS_USER": "", "ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE": ""},
                clear=False,
            ),
            mock.patch.object(RUNNER.HostStateBackup, "begin") as backup,
            mock.patch.object(RUNNER, "daemon_switch_result") as daemon_switch,
            mock.patch.object(
                RUNNER,
                "require_benchmark_account",
                side_effect=RUNNER.BenchmarkAccountError("manifest is missing"),
            ),
            mock.patch.object(RUNNER, "release_binary") as release_binary,
        ):
            with self.assertRaisesRegex(RUNNER.SmokeError, "benchmark-account preflight failed"):
                RUNNER.run_capacity(Path("/tmp/atm-capacity-unit"), Path("/tmp"), "tcp", 1)

        backup.assert_not_called()
        daemon_switch.assert_not_called()
        release_binary.assert_not_called()

    def test_managed_lifecycle_refuses_before_any_daemon_switch(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        with mock.patch.object(RUNNER, "daemon_switch_result") as daemon_switch:
            with self.assertRaisesRegex(RUNNER.SmokeError, "retired"):
                RUNNER.ManagedDaemonLifecycle(options).begin()

        daemon_switch.assert_not_called()

    def test_run_capacity_refuses_managed_daemon_before_any_host_mutation(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "atm-capacity-benchmark"
            primary = root / "primary" / ".atm" / "db"
            primary.mkdir(parents=True)
            mail_db = primary / "mail.db"
            mail_db.write_text("primary-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "release_binary") as release_binary,
                mock.patch.object(RUNNER, "daemon_switch_result") as daemon_switch,
                mock.patch.object(RUNNER, "require_clean_host_daemon_state") as clean,
            ):
                with self.assertRaisesRegex(RUNNER.SmokeError, "managed-daemon benchmarking is retired"):
                    RUNNER.run_capacity(
                        home, root, "tcp", 1, sample_count=1,
                        raw_evidence_directory=root, managed_daemon=options,
                    )

            self.assertEqual(mail_db.read_text(encoding="utf-8"), "primary-state")
            self.assertFalse(home.exists())

        clean.assert_not_called()
        daemon_switch.assert_not_called()
        release_binary.assert_not_called()

    def test_main_rejects_retired_managed_host_arguments_before_running_a_benchmark(self):
        with (
            mock.patch.object(sys, "argv", ["run_admission_capacity.py", "--managed-service", "x"]),
            mock.patch.object(RUNNER, "run_capacity") as run_capacity,
            contextlib.redirect_stderr(io.StringIO()),
        ):
            with self.assertRaises(SystemExit) as exit_error:
                RUNNER.main()

        self.assertEqual(exit_error.exception.code, 2)
        run_capacity.assert_not_called()

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_managed_lifecycle_quiesces_then_restores_original_state_and_pair(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        before = healthy_managed_status()
        calls: list[tuple[str, bool]] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append((action, doctor))
            return before

        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            state = os_home / ".atm" / "db"
            state.mkdir(parents=True)
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state") as clean,
            ):
                lifecycle = RUNNER.ManagedDaemonLifecycle(options)
                lifecycle.begin()
                self.assertFalse((state / "mail.db").exists())
                lifecycle.restore()

            self.assertEqual((state / "mail.db").read_text(encoding="utf-8"), "managed-state")

        clean.assert_called_once_with(smoke_label="admission-capacity smoke")
        self.assertEqual(
            calls,
            [("status", False), ("quiesce", False), ("restart", False), ("status", True)],
        )

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_managed_lifecycle_recovers_an_unavailable_pre_quiesce_daemon(self):
        """Initial selector capture must not require the very doctor recovery restores."""
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        pre_quiesce = healthy_managed_status()
        pre_quiesce.pop("doctor")
        after_restore = healthy_managed_status()
        calls: list[tuple[str, bool]] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append((action, doctor))
            if action == "status" and doctor:
                return after_restore
            return pre_quiesce

        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            state = os_home / ".atm"
            state.mkdir()
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
            ):
                lifecycle = RUNNER.ManagedDaemonLifecycle(options)
                lifecycle.begin()
                lifecycle.restore()

        self.assertEqual(
            calls,
            [("status", False), ("quiesce", False), ("restart", False), ("status", True)],
        )

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_managed_lifecycle_executes_and_restarts_the_selected_service_on_disposable_state(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        calls: list[tuple[str, bool]] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append((action, doctor))
            return healthy_managed_status()

        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            state = os_home / ".atm"
            state.mkdir()
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
            ):
                lifecycle = RUNNER.ManagedDaemonLifecycle(options)
                lifecycle.begin()
                lifecycle.start_isolated_service()
                lifecycle.restart_isolated_service()
                lifecycle.restore()

        self.assertEqual(
            calls,
            [
                ("status", False), ("quiesce", False),
                ("restart", False), ("status", True),
                ("quiesce", False), ("restart", False), ("status", True),
                ("quiesce", False), ("restart", False), ("status", True),
            ],
        )

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_backup_snapshot_failure_restarts_the_managed_pair(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        status = healthy_managed_status()
        calls: list[str] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append(action)
            return status

        with (
            mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
            mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
            mock.patch.object(RUNNER.HostStateBackup, "begin", side_effect=OSError("disk error")),
        ):
            with self.assertRaisesRegex(OSError, "disk error"):
                RUNNER.ManagedDaemonLifecycle(options).begin()

        self.assertEqual(calls, ["status", "quiesce", "restart", "status"])

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_restore_attempts_restart_and_doctor_even_when_state_restore_fails(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        calls: list[str] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append(action)
            return healthy_managed_status()

        lifecycle = RUNNER.ManagedDaemonLifecycle(
            options,
            backup=mock.Mock(restore=mock.Mock(side_effect=OSError("rename failed"))),
            pre_pair=RUNNER.selected_pair(healthy_managed_status()),
            quiesced=True,
        )
        with mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch):
            with self.assertRaisesRegex(RUNNER.SmokeError, "could not restore prior host ATM state"):
                lifecycle.restore()

        self.assertEqual(calls, ["restart", "status"])

    def test_daemon_switch_status_rejects_non_healthy_or_non_ready_doctor(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        for field, value, expected in (
            ("summary", {"status": "degraded"}, "not healthy"),
            ("runtime_status", {"readiness": "draining"}, "not ready"),
        ):
            status = healthy_managed_status()
            status["doctor"][field] = value
            command = {"exit_code": 0, "stdout": json.dumps(status), "stderr": ""}
            with mock.patch.object(RUNNER, "command_result", return_value=command):
                with self.assertRaisesRegex(RUNNER.SmokeError, expected):
                    RUNNER.daemon_switch_result("status", options, doctor=True)

    def test_daemon_switch_status_accepts_http_runtime_without_legacy_doctor_fields(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        status = healthy_managed_status()
        status["doctor"].pop("runtime_status")
        status["doctor"].pop("daemon_context")
        command = {"exit_code": 0, "stdout": json.dumps(status), "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=command):
            self.assertEqual(RUNNER.daemon_switch_result("status", options, doctor=True), status)

    def test_daemon_switch_timeout_covers_its_bounded_owner_repair_window(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        with mock.patch.object(
            RUNNER,
            "command_result",
            return_value={"exit_code": 0, "stdout": "", "stderr": ""},
        ) as command:
            self.assertEqual(RUNNER.daemon_switch_result("quiesce", options), {})

        self.assertEqual(
            command.call_args.kwargs["timeout"],
            RUNNER.MANAGED_DAEMON_TIMEOUT_SECONDS,
        )
        self.assertGreaterEqual(RUNNER.MANAGED_DAEMON_TIMEOUT_SECONDS, 100.0)

    def test_daemon_switch_status_requires_live_pair_proof(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        status = healthy_managed_status()
        status.pop("live_pair")
        command = {"exit_code": 0, "stdout": json.dumps(status), "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=command):
            with self.assertRaisesRegex(RUNNER.SmokeError, "selected release"):
                RUNNER.daemon_switch_result("status", options, doctor=True)

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_restore_surfaces_a_failed_doctor_after_state_is_put_back(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        status = healthy_managed_status()
        calls: list[str] = []

        def daemon_switch(action, _options, *, doctor=False):
            calls.append(action)
            if action == "status" and len(calls) > 3:
                raise RUNNER.SmokeError("managed daemon doctor failed: unavailable")
            return status

        with tempfile.TemporaryDirectory() as temp:
            os_home = Path(temp)
            state = os_home / ".atm"
            state.mkdir()
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            with (
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
            ):
                lifecycle = RUNNER.ManagedDaemonLifecycle(options)
                lifecycle.begin()
                with self.assertRaisesRegex(RUNNER.SmokeError, "could not restore managed daemon pair"):
                    lifecycle.restore()

            self.assertEqual((state / "mail.db").read_text(encoding="utf-8"), "managed-state")

        self.assertEqual(calls, ["status", "quiesce", "restart", "status"])

    @unittest.skip("retired: benchmarks must not operate the primary user's daemon or database")
    def test_benchmark_failure_restores_managed_state_and_verifies_doctor(self):
        options = RUNNER.ManagedDaemonOptions(service="com.example.atm")
        status = healthy_managed_status()
        calls: list[str] = []
        captured: dict[str, object] = {}

        def daemon_switch(action, _options, *, doctor=False):
            calls.append(action)
            return status

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            os_home = root / "os-home"
            state = os_home / ".atm"
            state.mkdir(parents=True)
            (state / "mail.db").write_text("managed-state", encoding="utf-8")
            home = root / "atm-capacity-benchmark"
            atm = root / "atm"
            daemon = root / "atm-daemon"
            atm.touch()
            daemon.touch()
            with (
                mock.patch.object(RUNNER, "select_host_state_isolation", return_value="backup_restore"),
                mock.patch.object(RUNNER, "os_account_home", return_value=os_home),
                mock.patch.object(RUNNER, "daemon_switch_result", side_effect=daemon_switch),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
                mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]),
                mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]),
                mock.patch.object(RUNNER, "runtime_environment", return_value={}),
                mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64),
                mock.patch.object(RUNNER, "prepare_capacity_roster"),
                mock.patch.object(RUNNER, "local_endpoint", side_effect=RUNNER.SmokeError("benchmark failed")),
                mock.patch.object(RUNNER, "write_raw_evidence", return_value=root / "raw.json"),
                mock.patch.object(
                    RUNNER,
                    "write_evidence",
                    side_effect=lambda _path, value: (captured.update(value), root / "evidence.json")[1],
                ),
            ):
                code, _evidence = RUNNER.run_capacity(
                    home, root, "tcp", 1, sample_count=1,
                    raw_evidence_directory=root, managed_daemon=options,
                )

            self.assertEqual((state / "mail.db").read_text(encoding="utf-8"), "managed-state")

        self.assertEqual(code, 1)
        self.assertEqual(captured["failure"], "benchmark failed")
        self.assertEqual(captured["managed_daemon_recovery"], "doctor-verified")
        self.assertEqual(
            calls,
            ["status", "quiesce", "restart", "status", "status", "quiesce", "restart", "status"],
        )

    def test_transport_is_platform_explicit(self):
        self.assertEqual(RUNNER.validate_transport("tcp"), "tcp")
        with mock.patch.object(RUNNER.os, "name", "posix"):
            self.assertEqual(RUNNER.validate_transport("uds"), "uds")
        with self.assertRaisesRegex(RUNNER.SmokeError, "must be"):
            RUNNER.validate_transport("https")
        with mock.patch.object(RUNNER.os, "name", "nt"):
            with self.assertRaisesRegex(RUNNER.SmokeError, "Windows"):
                RUNNER.validate_transport("uds")

    def test_runtime_environment_cannot_disable_the_hook(self):
        environment = RUNNER.runtime_environment(Path("/tmp/atm-capacity-unit"))
        self.assertNotIn("ATM_HTTP_RECEIVED_HOOK_MODE", environment)
        self.assertNotIn("ATM_HTTP_BENCHMARK_MODE", environment)
        self.assertEqual(environment["ATM_DAEMON_READY_STDOUT"], "1")

    def test_capacity_evidence_declares_whether_the_hook_was_measured(self):
        captured: dict[str, object] = {}
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory) / "atm-capacity-proof"
            daemon = Path(directory) / "atm-daemon"
            atm = Path(directory) / "atm"
            daemon.touch()
            atm.touch()
            with (
                mock.patch.object(
                    RUNNER,
                    "require_capacity_benchmark_account",
                    return_value=mock.Mock(durable_state_root=Path(directory) / ".atm" / "db"),
                ),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
                mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]),
                mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]),
                mock.patch.object(RUNNER, "runtime_environment", return_value={}),
                mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64),
                mock.patch.object(RUNNER, "prepare_capacity_roster"),
                mock.patch.object(RUNNER, "start_capacity_daemon", side_effect=RUNNER.SmokeError("stop after evidence setup")),
                mock.patch.object(RUNNER, "write_raw_evidence", return_value=Path(directory) / "raw.json"),
                mock.patch.object(
                    RUNNER,
                    "write_evidence",
                    side_effect=lambda _path, value: (captured.update(value), Path(directory) / "evidence.json")[1],
                ),
            ):
                _code, evidence_path = RUNNER.run_capacity(
                    home, Path(directory), "tcp", 1, sample_count=1,
                    raw_evidence_directory=Path(directory),
                )
        self.assertEqual(evidence_path.name, "evidence.json")
        self.assertEqual(captured["hook_mode"], "active")
        self.assertIn("active", captured["stages"]["post_commit_received_hook"])

    def test_failed_setup_records_a_schema_valid_duration(self):
        captured: dict[str, object] = {}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "atm-capacity-proof"
            daemon = root / "atm-daemon"
            atm = root / "atm"
            daemon.touch()
            atm.touch()
            with (
                mock.patch.object(
                    RUNNER,
                    "require_capacity_benchmark_account",
                    return_value=mock.Mock(durable_state_root=root / ".atm" / "db"),
                ),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
                mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]),
                mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]),
                mock.patch.object(RUNNER, "runtime_environment", return_value={}),
                mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64),
                mock.patch.object(RUNNER, "prepare_capacity_roster"),
                mock.patch.object(
                    RUNNER, "start_capacity_daemon", side_effect=RUNNER.SmokeError("setup failed"),
                ),
                mock.patch.object(RUNNER, "write_raw_evidence", return_value=root / "raw.json"),
                mock.patch.object(
                    RUNNER,
                    "write_evidence",
                    side_effect=lambda _path, value: (
                        compact_evidence(value), captured.update(value), root / "evidence.json"
                    )[-1],
                ),
            ):
                code, _evidence = RUNNER.run_capacity(
                    home, root, "tcp", 1, sample_count=1, raw_evidence_directory=root,
                )

        self.assertEqual(code, 1)
        self.assertIn("benchmark snapshot phase failed: setup failed", captured["failure"])
        self.assertIn("recovery:", captured["failure"])
        self.assertIsInstance(captured["run_duration_s"], float)
        self.assertGreaterEqual(captured["run_duration_s"], 0.0)

    def test_missing_plaintext_baseline_retains_measured_profile_before_failing(self):
        captured: dict[str, object] = {}
        profile = {
            "sample_count": 1,
            "target_duration_s": 1.0,
            "run_duration_s": 1.0,
            "intervals": complete_evidence()["runs"][0]["intervals"],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "atm-capacity-proof"
            daemon = root / "atm-daemon"
            atm = root / "atm"
            daemon.touch()
            atm.touch()
            process = mock.Mock(pid=123)
            daemon_output = mock.Mock()
            daemon_output.evidence.return_value = {}
            snapshot = mock.Mock(
                snapshot_id="snapshot-20260822T000000Z-0123456789abcdef",
                account_id="uid:999",
                user_version=1,
                page_count=1,
                byte_count=4096,
                sha256="0" * 64,
            )
            with (
                mock.patch.object(
                    RUNNER,
                    "require_capacity_benchmark_account",
                    return_value=mock.Mock(durable_state_root=root / ".atm" / "db"),
                ),
                mock.patch.object(RUNNER, "require_clean_host_daemon_state"),
                mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]),
                mock.patch.object(RUNNER, "release_binary", side_effect=[atm, daemon]),
                mock.patch.object(RUNNER, "runtime_environment", return_value={}),
                mock.patch.object(RUNNER, "regenerate_mtls_identity", return_value="a" * 64),
                mock.patch.object(RUNNER, "start_capacity_daemon", return_value=(process, daemon_output)) as start,
                mock.patch.object(RUNNER, "command_result", return_value={}),
                mock.patch.object(RUNNER, "benchmark_doctor_payload", return_value={}),
                mock.patch.object(RUNNER, "prepare_capacity_roster"),
                mock.patch.object(RUNNER, "local_endpoint", return_value=RUNNER.LocalEndpoint("tcp", "127.0.0.1:1")),
                mock.patch.object(RUNNER, "run_cached_roster_heartbeat_probe", return_value={}),
                mock.patch.object(RUNNER, "run_profile", return_value=profile),
                mock.patch.object(RUNNER, "release_version", return_value="atm test"),
                mock.patch.object(RUNNER, "reap_owned_daemon"),
                mock.patch.multiple(
                    RUNNER,
                    create_verified_snapshot=mock.DEFAULT,
                    restore_verified_snapshot=mock.DEFAULT,
                    verify_active_snapshot=mock.DEFAULT,
                ) as snapshot_api,
                mock.patch.object(RUNNER, "write_raw_evidence", return_value=root / "raw.json"),
                mock.patch.object(
                    RUNNER,
                    "write_evidence",
                    side_effect=lambda _path, value: (captured.update(value), root / "evidence.json")[1],
                ),
            ):
                snapshot_api["create_verified_snapshot"].return_value = snapshot
                snapshot_api["restore_verified_snapshot"].return_value = snapshot
                snapshot_api["verify_active_snapshot"].return_value = snapshot
                code, _evidence = RUNNER.run_capacity(
                    home, root, "tcp", 1, sample_count=1,
                    raw_evidence_directory=root,
                    preflight_failure_code=RUNNER.MISSING_PLAINTEXT_BASELINE,
                    preflight_failure="missing a complete passed same-host plaintext baseline",
                )

        self.assertEqual(code, 1)
        self.assertEqual(start.call_count, 2)
        self.assertEqual(captured["runs"], [profile])
        self.assertEqual(captured["clean_baseline_snapshot"]["snapshot_id"], snapshot.snapshot_id)
        self.assertEqual(captured["post_restore_snapshot"]["snapshot_id"], snapshot.snapshot_id)
        self.assertEqual(captured["failure"], "missing a complete passed same-host plaintext baseline")
        self.assertFalse(captured["passed"])

    def test_sparse_profiles_and_schema_fields_are_declared(self):
        self.assertEqual(RUNNER.SPARSE_FRAMES_PER_CONNECTION, (1, 2, 4, 8, 16, 64))

    def test_default_evidence_directory_is_the_public_summary_site(self):
        self.assertEqual(
            RUNNER.DEFAULT_EVIDENCE_DIR,
            RUNNER.ROOT / "site" / "reports" / "send-message-benchmark",
        )
        self.assertIn("site", RUNNER.DEFAULT_EVIDENCE_DIR.parts)
        self.assertIn("artifacts", RUNNER.DEFAULT_RAW_EVIDENCE_DIR.parts)

    def test_source_revision_requires_a_resolved_git_head(self):
        completed = mock.Mock(returncode=0, stdout="a" * 40 + "\n")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed):
            self.assertEqual(RUNNER.source_revision(), "a" * 40)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=mock.Mock(returncode=1, stdout="")):
            with self.assertRaisesRegex(RUNNER.SmokeError, "resolved HEAD"):
                RUNNER.source_revision()

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
        evidence = complete_evidence(
            frames_per_connection=16,
            messages_per_connection=16,
            decomposition={
                "async_storage_admission": {
                    "kind": "async_storage_admission",
                    "requested_count": 10_000,
                    "accepted_count": 10_000,
                    "worker_count": 64,
                    "elapsed_seconds": 0.2,
                    "admissions_per_second": 50_000.0,
                },
            },
        )
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            recorded = __import__("json").loads(path.read_text(encoding="utf-8"))

        self.assertEqual(recorded["schema_version"], 3)
        self.assertEqual(recorded["transport"], "tcp")
        self.assertEqual(recorded["frames_per_connection"], 16)
        self.assertEqual(
            recorded["direct_sqlite_message_write"]["admissions_per_second"],
            50_000.0,
        )

    def test_direct_writer_profile_requires_and_preserves_real_intervals(self):
        payload = {
            "kind": "canonical_core_write",
            "requested_count": 2_000,
            "accepted_count": 2_000,
            "worker_count": 64,
            "elapsed_seconds": 0.1,
            "admissions_per_second": 20_000.0,
            "intervals": [
                {
                    "requested_count": 1_000,
                    "accepted_count": 1_000,
                    "elapsed_seconds": 0.05,
                    "admissions_per_second": 20_000.0,
                },
                {
                    "requested_count": 1_000,
                    "accepted_count": 1_000,
                    "elapsed_seconds": 0.05,
                    "admissions_per_second": 20_000.0,
                },
            ],
        }
        completed = mock.Mock(returncode=0, stdout=json.dumps(payload), stderr="")
        roster = RUNNER.CapacityRoster("test", "team", "agent", "recipient")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            profile, measurement = RUNNER.run_direct_production_writer_profile(
                Path("/tmp/atm-daemon-benchmark"), {}, roster, 1_000, 2, 64,
            )
        self.assertEqual(measurement, payload)
        self.assertEqual(profile["sample_count"], 2)
        self.assertEqual(profile["intervals"][0]["admissions_per_second"], 20_000.0)
        self.assertIn("--direct-core-write", run.call_args.args[0])
        self.assertIn("--intervals", run.call_args.args[0])

    def test_profile_schema_distinguishes_minimum_from_actual_sample_count(self):
        interval = {"passed": True, "elapsed_seconds": 0.6}
        with mock.patch.object(RUNNER, "run_interval", return_value=interval):
            profile = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"), 1, 1_000, 2, 2,
                target_duration_seconds=1.0,
            )
        self.assertEqual(profile["minimum_sample_count"], 2)
        self.assertEqual(profile["sample_count"], 2)
        self.assertEqual(profile["target_duration_s"], 1.0)

    def test_evidence_filename_matches_the_published_benchmark_convention(self):
        evidence = complete_evidence(host_label="mac-arm64-01", frames_per_connection=16)
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.evidence_filename(Path(temp), evidence)
        self.assertEqual(path.name, "20260801-050000.123456-mac-arm64-01-tcp-f16.json")

    def test_evidence_filename_is_the_report_renderer_artifact_id(self):
        import benchmark_report

        evidence = complete_evidence(host_label="mac-arm64-01", transport="uds", frames_per_connection=8)
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            result = benchmark_report.load_result(path)
        self.assertEqual(path.stem, benchmark_report.result_id(result))

    def test_evidence_writer_redacts_host_private_fields_but_retains_endpoint_shape(self):
        evidence = complete_evidence(
            host_label="mac-arm64-01", transport="uds", atm_home="/Users/randlee/private/atm",
            doctor={"details": "/Users/randlee/.atm/logs/atm.log.jsonl"},
            endpoint={"transport": "uds", "address": "/Users/randlee/.atm/daemon.sock"},
        )
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            recorded = json.loads(path.read_text(encoding="utf-8"))
        self.assertNotIn("atm_home", recorded)
        self.assertNotIn("doctor", recorded)
        self.assertNotIn("endpoint", recorded)

    def test_published_doctor_status_is_compact(self):
        evidence = complete_evidence(
            host_label="mac-arm64-01", doctor={"host_private": "full diagnostics"},
            doctor_status="passed", doctor_after_restart={"status": "passed"},
        )
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            recorded = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(recorded["doctor_status"], "passed")
        self.assertEqual(recorded["doctor_after_restart_status"], "passed")

    def test_published_failure_is_redacted_by_the_summary_schema(self):
        evidence = complete_evidence(
            passed=False,
            failure="could not open /Users/randlee/.atm/private.db",
        )
        with tempfile.TemporaryDirectory() as temp:
            path = RUNNER.write_evidence(Path(temp), evidence)
            recorded = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(recorded["failure"], "could not open <redacted-path>")

    def test_thresholds_require_admission_and_optional_baseline(self):
        profile = {
            "intervals": [
                {"admissions_per_second": 1_100, "passed": True},
                {"admissions_per_second": 900, "passed": False},
            ]
        }
        thresholds = RUNNER.evaluate_profile_thresholds(profile, 950)
        self.assertEqual(thresholds["median_admissions_per_second"], 1_000)
        self.assertTrue(thresholds["baseline_passed"])
        self.assertFalse(thresholds["admission_passed"])
        self.assertFalse(thresholds["passed"])

    def test_thresholds_retain_a_explicit_transport_comparison_floor(self):
        profile = {"intervals": [{"admissions_per_second": 790, "passed": True}]}
        thresholds = RUNNER.evaluate_profile_thresholds(
            profile, None, comparison_median=1_000, comparison_ratio=0.75,
        )
        self.assertEqual(thresholds["comparison_target_admissions_per_second"], 750)
        self.assertTrue(thresholds["comparison_passed"])
        self.assertTrue(thresholds["passed"])

    def test_windows_comparison_is_reported_without_gating_windows_acceptance(self):
        profile = {"intervals": [{"admissions_per_second": 800, "passed": True}]}
        thresholds = RUNNER.evaluate_profile_thresholds(
            profile, None, comparison_median=2_000, comparison_ratio=0.75,
            comparison_required=False,
        )
        self.assertFalse(thresholds["comparison_passed"])
        self.assertFalse(thresholds["comparison_required"])
        self.assertTrue(thresholds["passed"])

    def test_matching_profile_reference_uses_one_complete_passed_ancestor_set(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for frame in RUNNER.TCP_COMPARISON_FRAMES:
                payload = {
                    "host_label": "mac-arm64-01",
                    "transport": "uds",
                    "peer_wire_security": "plaintext-test",
                    "execution_daemon": "shipped_atm_daemon",
                    "frames_per_connection": frame,
                    "source_revision": "b" * 40,
                    "generated_at": f"2026-08-01T00:00:{frame:02d}Z",
                    "passed": True,
                    "sample_count": 10,
                    "minimum_sample_count": 10,
                    "run_duration_s": 20.0,
                    "target_duration_s": 20.0,
                    "runs": [{"intervals": [{"admissions_per_second": frame * 1_000}]}],
                }
                (root / f"f{frame}.json").write_text(json.dumps(payload), encoding="utf-8")
            with mock.patch.object(RUNNER, "is_ancestor_revision", return_value=True):
                median, reference = RUNNER.matching_profile_reference(
                    root, "mac-arm64-01", "uds", 4, "c" * 40,
                )
        self.assertEqual(median, 4_000)
        self.assertEqual(reference, "b" * 40)

    def test_main_binds_the_validated_transport_before_selecting_profiles(self):
        with (
            mock.patch.object(sys, "argv", ["run_admission_capacity.py", "--diagnostic-only", "--transport", "invalid"]),
            mock.patch.object(RUNNER, "selected_profiles") as selected,
        ):
            with self.assertRaisesRegex(RUNNER.SmokeError, "must be `sqlite`, `uds`, or `tcp`"):
                RUNNER.main()
        selected.assert_not_called()

    def test_plain_benchmark_command_dispatches_the_required_four_target_matrix(self):
        with (
            mock.patch.object(sys, "argv", ["run_admission_capacity.py"]),
            mock.patch.object(RUNNER, "run_default_f8_matrix", return_value=0) as matrix,
        ):
            self.assertEqual(RUNNER.main(), 0)
        matrix.assert_called_once()

    def test_default_f8_matrix_runs_targets_in_fixed_order(self):
        observed: list[tuple[str, str | None]] = []

        def run_capacity(*_args, **kwargs):
            observed.append((kwargs["benchmark_target"], kwargs["peer_wire_security"]))
            return 0, Path("evidence.json")

        args = argparse.Namespace(
            atm_home=None,
            evidence_dir=Path("evidence"),
            raw_evidence_dir=Path("raw"),
            workers=64,
        )
        with mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity):
            self.assertEqual(RUNNER.run_default_f8_matrix(args), 0)
        self.assertEqual(
            observed,
            [
                ("sqlite", None),
                ("uds", "mutual-tls"),
                ("tcp", "plaintext-test"),
                ("tcp-tls", "mutual-tls"),
            ],
        )

    def test_plaintext_baseline_bootstrap_runs_the_complete_required_set(self):
        observed: list[tuple[int, bool, str, str]] = []

        def run_capacity(*_args, **kwargs):
            observed.append((
                _args[3], kwargs["comparison_required"], kwargs["peer_wire_security"],
                kwargs["benchmark_target"],
            ))
            return 0, Path("evidence.json")

        args = argparse.Namespace(
            atm_home=None,
            evidence_dir=Path("evidence"),
            raw_evidence_dir=Path("raw"),
            workers=64,
        )
        with mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity):
            self.assertEqual(RUNNER.run_plaintext_baseline_bootstrap(args), 0)
        self.assertEqual(
            observed,
            [(frame, False, "plaintext-test", "tcp") for frame in RUNNER.TCP_COMPARISON_FRAMES],
        )

    def test_plaintext_baseline_bootstrap_dispatches_only_when_explicit(self):
        with (
            mock.patch.object(
                sys,
                "argv",
                ["run_admission_capacity.py", "--bootstrap-plaintext-baseline"],
            ),
            mock.patch.object(RUNNER, "run_plaintext_baseline_bootstrap", return_value=0) as bootstrap,
        ):
            self.assertEqual(RUNNER.main(), 0)
        bootstrap.assert_called_once()

    def test_main_allows_windows_tcp_without_a_comparison_reference(self):
        captured: dict[str, object] = {}

        def run_capacity(*_args, **kwargs):
            captured.update(kwargs)
            return 0, mock.sentinel.evidence

        with tempfile.TemporaryDirectory() as directory:
            with (
                mock.patch.object(
                    sys,
                    "argv",
                    [
                        "run_admission_capacity.py",
                        "--diagnostic-only",
                        "--transport",
                        "tcp",
                        "--atm-home",
                        directory,
                        "--frames-per-connection",
                        "1",
                    ],
                ),
                mock.patch.object(RUNNER.os, "name", "nt"),
                # PureWindowsPath models argv path conversion without asking
                # the Windows host to instantiate an unsupported PosixPath.
                mock.patch.object(RUNNER, "Path", PureWindowsPath),
                mock.patch.object(RUNNER, "source_revision", return_value="a" * 40),
                mock.patch.object(
                    RUNNER,
                    "matching_profile_reference",
                    side_effect=RUNNER.SmokeError("missing comparison reference"),
                ) as comparison,
                mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity),
            ):
                self.assertEqual(RUNNER.main(), 0)

        comparison.assert_called_once()
        self.assertIsNone(captured["comparison_median"])
        self.assertFalse(captured["comparison_required"])

    def test_main_publishes_plaintext_baseline_gap_as_failure_evidence(self):
        captured: dict[str, object] = {}

        def run_capacity(*_args, **kwargs):
            captured.update(kwargs)
            return 1, mock.sentinel.evidence

        with tempfile.TemporaryDirectory() as directory:
            with (
                mock.patch.object(
                    sys,
                    "argv",
                    [
                        "run_admission_capacity.py",
                        "--diagnostic-only",
                        "--target",
                        "tcp",
                        "--atm-home",
                        directory,
                        "--frames-per-connection",
                        "1",
                    ],
                ),
                mock.patch.object(RUNNER, "source_revision", return_value="a" * 40),
                mock.patch.object(
                    RUNNER,
                    "matching_profile_reference",
                    side_effect=RUNNER.SmokeError("missing comparison reference"),
                ),
                mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity),
            ):
                self.assertEqual(RUNNER.main(), 1)

        self.assertEqual(
            captured["preflight_failure_code"],
            RUNNER.MISSING_PLAINTEXT_BASELINE,
        )
        self.assertIn("missing a complete passed same-host plaintext baseline", captured["preflight_failure"])

    def test_failed_summary_retains_the_plaintext_baseline_failure_code(self):
        summary = compact_evidence(complete_evidence(
            runs=[],
            passed=False,
            peer_wire_security="plaintext-test",
            benchmark_target="tcp",
            failure="no compatible baseline",
            benchmark_evidence_failure_code=RUNNER.MISSING_PLAINTEXT_BASELINE,
        ))
        self.assertEqual(
            summary.benchmark_evidence_failure_code,
            RUNNER.MISSING_PLAINTEXT_BASELINE,
        )

    def test_main_records_uds_baseline_source_as_a_diffable_comparison(self):
        captured: dict[str, object] = {}

        def run_capacity(*_args, **kwargs):
            captured.update(kwargs)
            return 0, evidence_path

        baseline = {
            "generated_at": "2026-08-01T00:00:00Z",
            "source_revision": "b" * 40,
            "transport": "uds",
            "frames_per_connection": 1,
            "passed": True,
            "sample_count": 10,
            "minimum_sample_count": 10,
            "run_duration_s": 20.0,
            "target_duration_s": 20.0,
            "runs": [{"intervals": [{"admissions_per_second": 1_000}]}],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            baseline_path = root / "baseline.json"
            evidence_path = root / "evidence.json"
            baseline_path.write_text(json.dumps(baseline), encoding="utf-8")
            evidence_path.write_text(json.dumps(baseline), encoding="utf-8")
            with (
                mock.patch.object(
                    sys,
                    "argv",
                    [
                        "run_admission_capacity.py",
                        "--diagnostic-only",
                        "--transport", "uds",
                        "--atm-home", str(root),
                        "--frames-per-connection", "1",
                        "--baseline", str(baseline_path),
                    ],
                ),
                mock.patch.object(RUNNER, "source_revision", return_value="a" * 40),
                mock.patch.object(RUNNER, "run_capacity", side_effect=run_capacity),
                # This verifies the portable UDS comparison schema. Keep the
                # Windows runtime guard covered by its dedicated unit test;
                # mocking only this validation preserves Windows `pathlib`.
                mock.patch.object(RUNNER, "validate_transport", return_value="uds"),
            ):
                self.assertEqual(RUNNER.main(), 0)

        self.assertEqual(captured["comparison_median"], 1_000)
        self.assertEqual(captured["comparison_source_revision"], "b" * 40)
        self.assertEqual(captured["comparison_host_label"], "local")

    def test_baseline_requires_matching_transport_and_frame_profile(self):
        payload = {
            "transport": "tcp",
            "frames_per_connection": 8,
            "passed": True,
            "sample_count": 10,
            "minimum_sample_count": 10,
            "run_duration_s": 20.0,
            "target_duration_s": 20.0,
            "runs": [{"intervals": [{"admissions_per_second": 1_000}]}],
        }
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "baseline.json"
            path.write_text(__import__("json").dumps(payload), encoding="utf-8")
            self.assertEqual(RUNNER.load_baseline_median(path, "tcp", 8), 1_000)
            with self.assertRaisesRegex(RUNNER.SmokeError, "transport"):
                RUNNER.load_baseline_median(path, "uds", 8)
            with self.assertRaisesRegex(RUNNER.SmokeError, "frames_per_connection"):
                RUNNER.load_baseline_median(path, "tcp", 16)

    def test_baseline_reference_retains_source_revision_and_observed_result(self):
        payload = {
            "generated_at": "2026-08-01T00:00:00Z",
            "source_revision": "a" * 40,
            "run_duration_s": 20.0,
            "passed": False,
            "runs": [{"intervals": [{"admissions_per_second": 180.0}]}],
        }
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "baseline.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            reference = RUNNER.baseline_reference(path)
        self.assertEqual(reference["source_revision"], "a" * 40)
        self.assertFalse(reference["passed"])
        self.assertEqual(reference["median_admissions_per_second"], 180.0)

    def test_invalid_baseline_is_rejected_before_its_median_is_used(self):
        payload = {"transport": "uds", "frames_per_connection": 1, "passed": False,
                   "sample_count": 1, "minimum_sample_count": 10, "run_duration_s": 0.1,
                   "target_duration_s": 20.0, "runs": [{"intervals": [{"admissions_per_second": 180.0}]}]}
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "baseline.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(RUNNER.SmokeError, "did not pass"):
                RUNNER.load_baseline_median(path, "uds", 1)

    def test_public_tcp_targets_select_only_the_ordinary_daemon_mode(self):
        self.assertEqual(
            RUNNER.resolve_benchmark_target("tcp", None),
            ("tcp", "plaintext-test", "tcp"),
        )
        self.assertEqual(
            RUNNER.resolve_benchmark_target("tcp-tls", "tcp"),
            ("tcp", "mutual-tls", "tcp-tls"),
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "requires transport"):
            RUNNER.resolve_benchmark_target("tcp-tls", "uds")

    def test_mode_aware_baseline_rejects_cross_security_comparison(self):
        payload = complete_evidence(
            transport="tcp",
            peer_wire_security="plaintext-test",
            execution_daemon="shipped_atm_daemon",
            frames_per_connection=8,
        )
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "baseline.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(
                RUNNER.load_baseline_median(path, "tcp", 8, "plaintext-test"), 1_000,
            )
            with self.assertRaisesRegex(RUNNER.SmokeError, "peer-wire baseline"):
                RUNNER.load_baseline_median(path, "tcp", 8, "mutual-tls")

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
        atm = Path(tempfile.gettempdir()) / "atm"
        capacity_home = Path(tempfile.gettempdir()) / "capacity-home"
        result = {"exit_code": 0, "stdout": "", "stderr": ""}
        with mock.patch.object(RUNNER, "command_result", return_value=result) as command:
            RUNNER.prepare_capacity_roster(
                atm, {"ATM_HOME": str(Path(tempfile.gettempdir()) / "atm-capacity-test")}, capacity_home
            )
        self.assertEqual(
            command.call_args_list[0].args[0],
            [
                str(atm), "teams", "add-member", "capacity-team", "capacity-agent",
                "--home-dir", str(capacity_home), "--json",
            ],
        )
        self.assertEqual(command.call_args_list[1].args[0][4], "capacity-recipient")
        self.assertEqual(len(command.call_args_list), 2)

    def test_capacity_roster_is_unique_per_profile(self):
        first = RUNNER.CapacityRoster.unique()
        second = RUNNER.CapacityRoster.unique()

        self.assertNotEqual(first.team, second.team)
        self.assertNotEqual(first.agent, second.agent)
        self.assertNotEqual(first.recipient, second.recipient)

    def test_custom_capacity_roster_flows_to_public_request(self):
        roster = RUNNER.CapacityRoster.unique()
        body = json.loads(
            RUNNER.http_request_body(Path("/tmp/atm-capacity-test"), 42, roster)
        )

        self.assertEqual(body["caller_identity"], roster.agent)
        self.assertEqual(body["caller_team"], roster.team)
        self.assertEqual(
            body["to"], {"agent": roster.recipient, "team": roster.team}
        )

    def test_cached_roster_heartbeat_body_targets_the_warmed_capacity_member(self):
        body = json.loads(RUNNER.cached_roster_heartbeat_body(17))
        self.assertEqual(body["team"], "capacity-team")
        self.assertEqual(body["member"], "capacity-agent")
        self.assertEqual(body["pid"], 90_017)
        self.assertEqual(body["activity"], "active_tool_use")
        self.assertTrue(body["observed_at"].endswith("Z"))

    def test_cached_roster_probe_warms_once_then_records_a_no_sqlite_profile(self):
        warmup = RUNNER.AdmissionResult(status=200, elapsed_ms=0.1)
        profile = {"passed": True, "operation": "cached_roster_heartbeat"}
        endpoint = RUNNER.LocalEndpoint("uds", "/tmp/atm.sock")
        with (
            mock.patch.object(RUNNER, "submit_connection", return_value=[warmup]) as submit,
            mock.patch.object(RUNNER, "run_profile", return_value=profile) as run_profile,
        ):
            result = RUNNER.run_cached_roster_heartbeat_probe(endpoint, Path("/tmp/home"), 1, 8)

        self.assertEqual(result["warmup"], {"status": 200, "passed": True})
        self.assertIn("no SQLite reads", result["storage"])
        request = submit.call_args.args[1][0]
        self.assertEqual(request.path, "/v1/atm/heartbeat")
        self.assertEqual(request.expected_status, 200)
        self.assertEqual(run_profile.call_args.kwargs["operation"], "cached_roster_heartbeat")
        self.assertEqual(run_profile.call_args.kwargs["minimum_admissions_per_second"], 0)

    def test_interval_preserves_the_first_failure_and_requires_all_1000_responses(self):
        calls = 0
        calls_lock = threading.Lock()

        def submit(_sequence, _message_count):
            nonlocal calls
            with calls_lock:
                calls += 1
                status = 201 if calls != 7 else 503
            return [RUNNER.AdmissionResult(status, 0.1, None if status == 201 else "HTTP 503")]

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

    def test_connection_worker_limit_reserves_descriptors_under_a_posix_soft_limit(self):
        limited_resource = mock.Mock()
        limited_resource.RLIMIT_NOFILE = 7
        limited_resource.RLIM_INFINITY = -1
        limited_resource.getrlimit.return_value = (256, 256)
        with mock.patch.object(RUNNER, "resource", limited_resource):
            self.assertEqual(RUNNER.admission_connection_worker_limit(512), 192)

    def test_connection_worker_limit_keeps_requested_workers_without_rlimit_support(self):
        with mock.patch.object(RUNNER, "resource", None):
            self.assertEqual(RUNNER.admission_connection_worker_limit(512), 512)

    def test_connection_worker_limit_keeps_requested_workers_for_an_unbounded_limit(self):
        unlimited_resource = mock.Mock()
        unlimited_resource.RLIMIT_NOFILE = 7
        unlimited_resource.RLIM_INFINITY = -1
        unlimited_resource.getrlimit.return_value = (-1, -1)
        with mock.patch.object(RUNNER, "resource", unlimited_resource):
            self.assertEqual(RUNNER.admission_connection_worker_limit(512), 512)

    def test_interval_uses_the_published_application_wire_metric_names(self):
        result = RUNNER.run_interval(
            lambda _sequence, message_count: [
                RUNNER.AdmissionResult(201, 0.1, None, request_bytes=10, response_bytes=5)
                for _ in range(message_count)
            ],
            0,
            1,
            1,
            1,
        )
        self.assertIn("request_frames_per_second", result)
        self.assertIn("application_wire_bytes", result)
        self.assertIn("application_wire_bytes_per_second", result)
        self.assertNotIn("http_request_frames_per_second", result)
        self.assertNotIn("wire_bytes", result)

    def test_interval_latency_p50_uses_schema_distribution_for_even_samples(self):
        latencies = iter((1.0, 2.0, 3.0, 4.0))

        def submit(_sequence, _message_count):
            return [RUNNER.AdmissionResult(201, next(latencies))]

        result = RUNNER.run_interval(submit, 0, 1, 1, 4)
        self.assertEqual(result["latency_ms"]["p50"], 2.5)

    def test_profile_median_uses_schema_distribution_for_even_samples(self):
        profile = {
            "intervals": [
                {"admissions_per_second": value}
                for value in (1.0, 2.0, 3.0, 4.0)
            ]
        }
        self.assertEqual(RUNNER.profile_median_admissions_per_second(profile), 2.5)

    def test_interval_metrics_are_retained_by_the_benchmark_report_schema(self):
        import benchmark_report

        interval = RUNNER.run_interval(
            lambda _sequence, message_count: [
                RUNNER.AdmissionResult(201, 0.1, None, request_bytes=10, response_bytes=5)
                for _ in range(message_count)
            ],
            0,
            1,
            1,
            1,
        )
        payload = {
            "schema_version": 2,
            "generated_at": "2026-08-01T05:00:00Z",
            "host_label": "test-host",
            "transport": "uds",
            "frames_per_connection": 1,
            "run_duration_s": interval["elapsed_seconds"],
            "runs": [{"intervals": [interval]}],
            "passed": interval["passed"],
        }
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "result.json"
            source.write_text(json.dumps(payload), encoding="utf-8")
            rendered = benchmark_report.load_result(source)
        recorded = rendered["metrics"]
        self.assertIn("request_frames_per_second", recorded)
        self.assertIn("application_wire_bytes", recorded)

    def test_profile_retains_each_requested_interval_in_evidence(self):
        with mock.patch.object(
            RUNNER, "run_interval", return_value={"passed": True, "elapsed_seconds": 1.0}
        ) as interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"),
                2,
                10_000,
                3,
                2,
                target_duration_seconds=3.0,
            )
        self.assertEqual(len(result["intervals"]), 3)
        self.assertTrue(result["passed"])
        self.assertEqual(interval.call_count, 3)
        self.assertEqual(interval.call_args.args[2:], (2, 2, 10_000))

    def test_profile_extends_past_ten_intervals_until_the_sustained_duration(self):
        interval = {"passed": True, "elapsed_seconds": 0.4}
        with mock.patch.object(RUNNER, "run_interval", return_value=interval) as run_interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"), 1, 1_000, 10, 2,
                target_duration_seconds=1.0,
            )
        self.assertEqual(result["minimum_sample_count"], 10)
        self.assertEqual(result["sample_count"], 10)
        self.assertAlmostEqual(result["run_duration_s"], 4.0)
        self.assertEqual(run_interval.call_count, 10)

    def test_profile_continues_after_minimum_intervals_until_target_duration(self):
        interval = {"passed": True, "elapsed_seconds": 0.4}
        with mock.patch.object(RUNNER, "run_interval", return_value=interval) as run_interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"), 1, 1_000, 2, 2,
                target_duration_seconds=1.0,
            )
        self.assertEqual(result["sample_count"], 3)
        self.assertAlmostEqual(result["run_duration_s"], 1.2)
        self.assertEqual(run_interval.call_count, 3)

    def test_profile_stops_at_the_first_failed_interval(self):
        interval = {"passed": False, "elapsed_seconds": 0.1}
        with mock.patch.object(RUNNER, "run_interval", return_value=interval) as run_interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/socket"),
                Path("/tmp/atm-capacity-test"), 1, 1_000, 10, 2,
            )
        self.assertFalse(result["passed"])
        self.assertEqual(result["sample_count"], 1)
        self.assertEqual(run_interval.call_count, 1)

    def test_profile_retains_clean_under_threshold_intervals(self):
        interval = {"passed": False, "error_free": True, "elapsed_seconds": 0.4}
        with mock.patch.object(RUNNER, "run_interval", return_value=interval) as run_interval:
            result = RUNNER.run_profile(
                RUNNER.LocalEndpoint("uds", "/tmp/atm-capacity-test"),
                Path("/tmp/atm-capacity-test"), 64, 1_000, 2, 2,
                target_duration_seconds=1.0,
            )
        self.assertFalse(result["passed"])
        self.assertEqual(result["sample_count"], 3)
        self.assertEqual(run_interval.call_count, 3)

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

    def test_failed_daemon_readiness_reaps_the_new_child(self):
        process = mock.Mock()
        output = mock.Mock()
        with (
            mock.patch.object(RUNNER.subprocess, "Popen", return_value=process),
            mock.patch.object(RUNNER.DaemonOutputCapture, "start", return_value=output),
            mock.patch.object(RUNNER, "await_daemon_ready", side_effect=RUNNER.SmokeError("not ready")),
            mock.patch.object(RUNNER, "reap_owned_daemon") as reap,
        ):
            with self.assertRaisesRegex(RUNNER.SmokeError, "not ready"):
                RUNNER.start_capacity_daemon(
                    Path("/tmp/daemon"), Path("/tmp"), {}, "mutual-tls"
                )
        reap.assert_called_once_with(process)
        output.join.assert_called_once_with()

    def test_capacity_daemon_launches_the_shipped_binary_with_only_peer_wire_mode(self):
        process = mock.Mock()
        process.stdout = mock.Mock()
        process.stderr = mock.Mock()
        output = mock.Mock()
        with (
            mock.patch.object(RUNNER.subprocess, "Popen", return_value=process) as popen,
            mock.patch.object(RUNNER.DaemonOutputCapture, "start", return_value=output),
            mock.patch.object(RUNNER, "await_daemon_ready"),
        ):
            RUNNER.start_capacity_daemon(
                Path("/release/atm-daemon"), Path("/tmp/atm-capacity-proof"), {},
                "plaintext-test",
            )
        launched = popen.call_args.args[0]
        self.assertEqual(Path(launched[0]), Path("/release/atm-daemon"))
        self.assertEqual(launched[1:], ["--peer-wire-security", "plaintext-test"])

    def test_managed_mode_rejects_a_doctor_that_reports_the_wrong_wire_mode(self):
        status = healthy_managed_status()
        status["doctor"]["daemon_context"]["peer_wire_security"] = "mutual-tls"
        with self.assertRaisesRegex(RUNNER.SmokeError, "expected plaintext-test"):
            RUNNER.require_managed_peer_wire_security(status, "plaintext-test")

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

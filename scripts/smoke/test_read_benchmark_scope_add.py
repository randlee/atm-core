from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from scripts.smoke import read_benchmark as benchmark
from scripts.smoke import benchmark_account
from scripts.smoke.benchmark_schema import BaselineEntry, BaselineSet


def _baselines() -> benchmark.BaselineSet:
    entries = tuple(
        BaselineEntry(
            host_label="test-host",
            target=family.family_id,
            p50_floor=100.0,
            approved_by="test",
            effective_from="2026-08-31T00:00:00Z",
            seeded_runs=3,
            source_campaigns=("one", "two", "three"),
            corpus_seed=benchmark.CORPUS_SEED,
            corpus_generator_version=benchmark.CORPUS_GENERATOR_VERSION,
            harness_version=benchmark.HARNESS_VERSION,
            fanout=benchmark.FANOUT,
            mailbox_pool_size=benchmark.MAILBOX_POOL_SIZE,
            mailbox_queue_depth=benchmark.MAILBOX_QUEUE_DEPTH,
            search_pool_size=benchmark.SEARCH_POOL_SIZE,
            search_queue_depth=benchmark.SEARCH_QUEUE_DEPTH,
        )
        for family in benchmark.FAMILIES
    )
    return BaselineSet(revision=2, entries=entries)


class ReadBenchmarkScopeAddTests(unittest.TestCase):
    def test_lane_settings_are_parsed_only_from_daemon_report(self) -> None:
        payload = {
            "reader_lanes": {
                "mailbox": {"pool_size": 4, "queue_depth": 16},
                "search": {"pool_size": 2, "queue_depth": 8},
            }
        }
        with mock.patch.object(
            benchmark,
            "_run",
            return_value=subprocess.CompletedProcess(
                ["atm", "doctor", "--json"], 0, json.dumps(payload), ""
            ),
        ):
            settings = benchmark.load_effective_lane_settings("atm", {})
        benchmark.validate_effective_lane_settings(benchmark.FAMILIES, settings)
        self.assertEqual(settings["mailbox"]["pool_size"], 4)

    def test_missing_lane_report_fails_closed(self) -> None:
        with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "does not expose"):
            benchmark._parse_effective_lane_settings({"status": "ready"})

    def test_lane_mismatch_fails_closed(self) -> None:
        settings = {
            "mailbox": {"pool_size": 1, "queue_depth": 16},
            "search": {"pool_size": 2, "queue_depth": 8},
        }
        with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "do not satisfy D7"):
            benchmark.validate_effective_lane_settings(benchmark.FAMILIES, settings)

    def test_missing_and_malformed_ratchet_state_are_explicit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baselines.previous.json"
            with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "missing baselines"):
                benchmark.load_previous_baselines(path)
            path.write_text("not-json", encoding="utf-8")
            with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "malformed ratchet state"):
                benchmark.load_previous_baselines(path)

    def test_ratchet_engages_at_observed_p50_minus_tolerance_without_lowering(self) -> None:
        baseline = _baselines()
        results = [
            {
                "family": family.family_id,
                "status": "PASS",
                "throughput_per_second": {"p50": 120.0},
            }
            for family in benchmark.FAMILIES
        ]
        record = benchmark.engage_ratchet(results, baseline, baseline, "test-host")
        self.assertTrue(record["engaged"])
        self.assertEqual(record["tolerance_pct"], benchmark.RATCHET_TOLERANCE_PCT)
        self.assertEqual(record["entries"][0]["candidate_floor"], 114.0)
        self.assertEqual(record["entries"][0]["proposed_floor"], 114.0)

    def test_floor_breach_still_emits_failed_campaign_and_envelope(self) -> None:
        baseline = _baselines()
        identity = benchmark.ExecutionIdentity("runner", 501, "/tmp/runner", "test-host")
        failed_results = [
            {
                "family": family.family_id,
                "status": "FAIL",
                "fanout": benchmark.FANOUT,
                "pool_size": family.pool_size,
                "queue_depth": family.queue_depth,
                "throughput_per_second": {
                    "min": 50.0, "p50": 50.0, "p95": 50.0, "p99": 50.0, "max": 50.0,
                },
                "latency_ms": {
                    "min": 1.0, "p50": 1.0, "p95": 1.0, "p99": 1.0, "max": 1.0,
                },
                "requests": {"total": 1, "successful": 0, "success_rate": 0.0},
                "throughput_requests_per_second": 0.0,
                "diagnostics": {},
            }
            for family in benchmark.FAMILIES
        ]
        lane_settings = {
            "mailbox": {"pool_size": 4, "queue_depth": 16},
            "search": {"pool_size": 2, "queue_depth": 8},
        }
        with tempfile.TemporaryDirectory() as directory:
            report_dir = Path(directory) / "read-query-benchmark"
            reports_root = Path(directory) / "reports"
            reports_root.mkdir()
            envelope_path = reports_root / "read-query-benchmark.json"
            def compose_report(_template: Path, _variables: dict[str, object], output: Path) -> None:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text("<html></html>", encoding="utf-8")

            with mock.patch.dict(os.environ, {"ATM_CAPACITY_HOST_LABEL": "test-host"}), mock.patch.object(
                benchmark, "REPORT_DIR", report_dir
            ), mock.patch.object(
                benchmark, "REPORTS_ROOT", reports_root
            ), mock.patch.object(benchmark, "REPORT_ENVELOPE", envelope_path), mock.patch.object(
                benchmark, "capture_execution_identity", return_value=identity
            ), mock.patch.object(benchmark, "load_baselines", return_value=baseline), mock.patch.object(
                benchmark, "load_previous_baselines", return_value=baseline
            ), mock.patch.object(benchmark, "_check_baseline_revision"), mock.patch.object(
                benchmark_account, "require_benchmark_account", return_value=object()
            ), mock.patch.object(benchmark, "_atm_binary", return_value="atm"), mock.patch.object(
                benchmark, "load_effective_lane_settings", return_value=lane_settings
            ), mock.patch.object(benchmark, "validate_effective_lane_settings"), mock.patch.object(
                benchmark, "deterministic_corpus", return_value=benchmark.deterministic_corpus()
            ), mock.patch.object(benchmark, "prepare_corpus"), mock.patch.object(
                benchmark, "run_family", side_effect=failed_results
            ), mock.patch.object(benchmark, "_source_revision", return_value="a" * 40), mock.patch.object(
                benchmark, "compose", side_effect=compose_report
            ), mock.patch.object(benchmark, "regenerate_index"), mock.patch.object(
                benchmark.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")
            ):
                self.assertEqual(benchmark.execute(list(benchmark.FAMILY_BY_ID)), 1)
            payloads = [json.loads(path.read_text(encoding="utf-8")) for path in report_dir.glob("*.json")]
            campaign = next(payload for payload in payloads if len(payload["families"]) == 3)
            self.assertEqual(campaign["status"], "FAIL")
            self.assertFalse(campaign["ratchet"]["engaged"])
            self.assertTrue(envelope_path.is_file())


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from scripts.smoke import read_benchmark as benchmark
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


if __name__ == "__main__":
    unittest.main()

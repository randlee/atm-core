from __future__ import annotations

import unittest
from datetime import datetime, timezone
from unittest import mock

from scripts.smoke import read_benchmark as benchmark


class ReadBenchmarkContractTests(unittest.TestCase):
    def test_registry_contains_exact_three_families_and_fixed_lane_defaults(self) -> None:
        self.assertEqual(
            tuple(family.family_id for family in benchmark.FAMILIES),
            ("read-fanout", "query-fts", "read-under-write-load"),
        )
        self.assertEqual(
            (benchmark.MAILBOX_POOL_SIZE, benchmark.MAILBOX_QUEUE_DEPTH), (4, 16)
        )
        self.assertEqual(
            (benchmark.SEARCH_POOL_SIZE, benchmark.SEARCH_QUEUE_DEPTH), (2, 8)
        )
        self.assertEqual(benchmark.FANOUT, 32)

    def test_corpus_is_deterministic_and_meets_d7_shape(self) -> None:
        first = benchmark.deterministic_corpus()
        second = benchmark.deterministic_corpus()
        self.assertEqual(first, second)
        self.assertEqual(len(first.members), 32)
        self.assertEqual(len({member.team for member in first.members}), 8)
        team_counts = {
            sum(member.team == team for member in first.members)
            for team in {member.team for member in first.members}
        }
        self.assertEqual(team_counts, {4})

    def test_corpus_contract_rejects_skewed_team_distribution(self) -> None:
        family = benchmark.FAMILIES[0]
        members = tuple(
            benchmark.CorpusMember("team-a" if index < 25 else f"team-{index - 24}", f"agent-{index:02d}")
            for index in range(32)
        )
        skewed = benchmark.Corpus("seed", "generator", members)
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family,
                corpus=skewed,
                fanout=benchmark.FANOUT,
                warmup_seconds=1,
                measurement_seconds=1,
            )

    def test_corpus_payload_size_and_official_cutover_gate_are_explicit(self) -> None:
        body = benchmark._fixed_body("seed", benchmark.CORPUS_PAYLOAD_BYTES)
        self.assertEqual(len(body.encode("utf-8")), benchmark.CORPUS_PAYLOAD_BYTES)
        with mock.patch.object(benchmark, "AV1B_CUTOVER_LANDED", False):
            with self.assertRaises(benchmark.ReadBenchmarkError):
                benchmark.require_av1b_cutover()
        with mock.patch.object(benchmark, "AV1B_CUTOVER_LANDED", True):
            benchmark.require_av1b_cutover()

    def test_missing_seed_warmup_and_partial_artifacts_fail_closed(self) -> None:
        family = benchmark.FAMILIES[0]
        corpus = benchmark.deterministic_corpus()
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family, corpus=benchmark.Corpus("", corpus.generator_version, corpus.members),
                fanout=benchmark.FANOUT, warmup_seconds=1, measurement_seconds=1,
            )
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family, corpus=corpus, fanout=benchmark.FANOUT,
                warmup_seconds=0, measurement_seconds=1,
            )
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family, corpus=corpus, fanout=benchmark.FANOUT,
                warmup_seconds=1, measurement_seconds=1, partial_artifact=True,
            )

    def test_success_rate_gate_and_shared_distribution_are_unrounded(self) -> None:
        family = benchmark.FAMILIES[0]
        corpus = benchmark.deterministic_corpus()
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family, corpus=corpus, fanout=benchmark.FANOUT,
                warmup_seconds=1, measurement_seconds=1, success_rate=0.998,
            )
        metrics = benchmark.distribution([100.123456, 101.123456, 99.123456])
        self.assertEqual(metrics["p50"], 100.123456)

    def test_metric_distribution_contains_tail_percentiles(self) -> None:
        metrics = benchmark.distribution([1.0, 2.0, 3.0, 4.0, 5.0])
        self.assertEqual(metrics["p50"], 3.0)
        self.assertEqual(metrics["p95"], 5.0)
        self.assertEqual(metrics["p99"], 5.0)

    def test_timed_out_observation_is_counted_in_diagnostics(self) -> None:
        diagnostics = benchmark._diagnostics(
            benchmark.FAMILIES[0],
            [benchmark.RequestObservation(12.0, False, timed_out=True, error="deadline")],
        )
        self.assertEqual(diagnostics["deadline_expiries"]["observed_cli_timeouts"], 1)
        self.assertIsNone(diagnostics["deadline_expiries"]["expired_in_queue"]["value"])
        self.assertEqual(diagnostics["saturation_events"]["status"], "unimplemented")

    def test_floor_requires_three_run_provenance(self) -> None:
        result = {
            "throughput_per_second": {"p50": 100.0},
            "latency_ms": {"p50": 2.0},
            "status": "PASS",
        }
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.apply_floor(result, benchmark.BaselineEntry(
                host_label="m5-atmbench",
                target="read-fanout",
                p50_floor=90.0,
                approved_by="quality-mgr",
                effective_from=datetime.now(timezone.utc),
            ))


if __name__ == "__main__":
    unittest.main()

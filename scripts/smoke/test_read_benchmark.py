from __future__ import annotations

import unittest

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

    def test_success_rate_gate_and_ratchet_are_unrounded(self) -> None:
        family = benchmark.FAMILIES[0]
        corpus = benchmark.deterministic_corpus()
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.validate_workload_contract(
                family=family, corpus=corpus, fanout=benchmark.FANOUT,
                warmup_seconds=1, measurement_seconds=1, success_rate=0.998,
            )
        point = benchmark.ratchet_floor(
            family_id=family.family_id,
            host_label="m5-atmbench",
            campaign_p50s=(100.123456, 101.123456, 99.123456),
            source_campaigns=("a", "b", "c"),
            corpus=corpus,
        )
        self.assertEqual(point["p50_floor"], 99.123456 * 0.95)
        raised = benchmark.ratchet_floor(
            family_id=family.family_id,
            host_label="m5-atmbench",
            campaign_p50s=(100, 101, 102),
            source_campaigns=("d", "e", "f"),
            corpus=corpus,
            previous=point,
        )
        self.assertGreaterEqual(raised["p50_floor"], point["p50_floor"])

    def test_metric_distribution_contains_tail_percentiles(self) -> None:
        metrics = benchmark.metric_distribution([1.0, 2.0, 3.0, 4.0, 5.0])
        self.assertEqual(metrics["p50"], 3.0)
        self.assertEqual(metrics["p95"], 4.0)
        self.assertEqual(metrics["p99"], 4.0)

    def test_floor_requires_three_run_provenance(self) -> None:
        result = {
            "throughput_per_second": {"p50": 100.0},
            "latency_ms": {"p50": 2.0},
            "status": "PASS",
        }
        with self.assertRaises(benchmark.ReadBenchmarkError):
            benchmark.apply_floor(result, {"p50_floor": 90.0})


if __name__ == "__main__":
    unittest.main()

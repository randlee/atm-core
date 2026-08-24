"""Contract tests for the AO2.10 benchmark v4 data model."""
from __future__ import annotations

from datetime import datetime, timezone
import unittest

from pydantic import ValidationError

from scripts.smoke.benchmark_schema import (
    BaselineEntry,
    BaselineRef,
    BaselineSet,
    BenchmarkCampaign,
    BenchmarkMetrics,
    BenchmarkRunResult,
    DurabilityAfterRestart,
    MetricDistribution,
    WireBytes,
    artifact_id,
    campaign_id,
    require_non_decreasing_baselines,
)


NOW = datetime(2026, 8, 24, tzinfo=timezone.utc)


def distribution() -> MetricDistribution:
    return MetricDistribution(min=1, p50=20_000, p95=21_000, p99=21_500, max=22_000)


def metrics(*, network: bool) -> BenchmarkMetrics:
    data = {
        "interval_count": 2,
        "passed_interval_count": 2,
        "accepted_count": 10,
        "requested_count": 10,
        "response_count": 10,
        "admissions_per_second": distribution(),
        "time_to_send_1k_s": distribution(),
        "interval_latency_ms": distribution(),
    }
    if network:
        data.update({
            "connection_count": 2,
            "application_wire_bytes": WireBytes(request=2, response=3, total=5),
            "request_frames_per_second": distribution(),
            "connections_per_second": distribution(),
            "application_wire_bytes_per_second": distribution(),
        })
    return BenchmarkMetrics.model_validate(data)


def result(target: str = "tcp", **overrides: object) -> BenchmarkRunResult:
    values: dict[str, object] = {
        "campaign_id": "20260824T000000Z-rand-m5",
        "host_label": "rand-m5",
        "os": "macos",
        "target": target,
        "status": "PASS",
        "incomplete_reason": None,
        "generated_at": NOW,
        "source_revision": "a" * 40,
        "binary_hashes": {"atm-daemon": "b" * 64},
        "frames_per_connection": 0 if target == "sqlite" else 8,
        "messages_requested": 10,
        "messages_admitted": 10,
        "messages_durable": 10,
        "metrics": metrics(network=target != "sqlite"),
        "baseline": BaselineRef(revision=1, p50_floor=17_500),
        "durability_after_restart": DurabilityAfterRestart(
            expected_accepted_count=10, observed_mailbox_count=10, passed=True,
        ),
    }
    values.update(overrides)
    return BenchmarkRunResult.model_validate(values)


class BenchmarkSchemaV4Tests(unittest.TestCase):
    def test_complete_durable_result_above_floor_is_pass(self) -> None:
        self.assertEqual(result().status, "PASS")

    def test_non_durable_complete_result_is_fail(self) -> None:
        self.assertEqual(result(messages_durable=9, status="FAIL").status, "FAIL")

    def test_complete_result_below_its_host_target_floor_is_fail(self) -> None:
        below_floor = result(
            status="FAIL",
            baseline=BaselineRef(revision=1, p50_floor=20_001),
        )
        self.assertEqual(below_floor.status, "FAIL")

    def test_missing_lifecycle_is_incomplete_with_reason(self) -> None:
        incomplete = result(
            status="INCOMPLETE", incomplete_reason="post-restart durability missing",
            durability_after_restart=None,
        )
        self.assertEqual(incomplete.status, "INCOMPLETE")

    def test_stored_status_cannot_override_policy(self) -> None:
        with self.assertRaises(ValidationError):
            result(messages_durable=9)

    def test_required_target_missing_forces_incomplete_campaign(self) -> None:
        tcp = result()
        campaign = BenchmarkCampaign(
            campaign_id=tcp.campaign_id,
            host_label=tcp.host_label,
            os="macos",
            phase="ao2",
            started_at=NOW,
            completed_at=NOW,
            source_revision=tcp.source_revision,
            results=(tcp,),
            status="INCOMPLETE",
        )
        self.assertEqual(campaign.status, "INCOMPLETE")

    def test_sqlite_rejects_invented_network_metrics(self) -> None:
        with self.assertRaises(ValidationError):
            result("sqlite", metrics=metrics(network=True))

    def test_baseline_ratchet_rejects_lower_floor(self) -> None:
        old = BaselineSet(revision=1, entries=(BaselineEntry(
            host_label="rand-m5", target="tcp", p50_floor=17_500,
            approved_by="quality-mgr", effective_from=NOW,
        ),))
        lowered = BaselineSet(revision=2, entries=(BaselineEntry(
            host_label="rand-m5", target="tcp", p50_floor=17_499,
            approved_by="quality-mgr", effective_from=NOW,
        ),))
        with self.assertRaisesRegex(ValueError, "may not lower"):
            require_non_decreasing_baselines(old, lowered)

    def test_campaign_and_target_ids_are_utc_safe_and_shared(self) -> None:
        identifier = campaign_id(started_at=NOW, host_label="rand-m5")
        self.assertEqual(identifier, "20260824T000000Z-rand-m5")
        self.assertEqual(
            artifact_id(campaign_id=identifier, target="tcp-tls"),
            "20260824T000000Z-rand-m5-tcp-tls",
        )


if __name__ == "__main__":
    unittest.main()

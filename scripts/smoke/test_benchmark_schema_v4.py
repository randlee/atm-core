"""Contract tests for the AO2.10 benchmark v4 data model."""
from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import unittest

from pydantic import ValidationError

from scripts.smoke.benchmark_schema import (
    BaselineEntry,
    BaselineRef,
    BaselineSet,
    BenchmarkCampaign,
    BenchmarkMetrics,
    BenchmarkRunResult,
    BenchmarkSummary,
    DurabilityAfterRestart,
    HistoricalCampaignEntry,
    HistoricalRecord,
    HistoricalResultEntry,
    MetricDistribution,
    RatchetPoint,
    UnattributedEntry,
    WireBytes,
    artifact_id,
    campaign_id,
    require_non_decreasing_baselines,
)
from scripts.smoke.benchmark_baselines import load_baselines


NOW = datetime(2026, 8, 24, tzinfo=timezone.utc)
ROOT = Path(__file__).resolve().parents[2]


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

    def test_all_required_targets_present_with_a_fail_and_no_incomplete_is_fail(self) -> None:
        """The pure-FAIL roll-up branch: previously only exercised implicitly
        alongside an INCOMPLETE target in the precedence test below."""
        sqlite = result("sqlite")
        uds = result("uds")
        tcp = result(
            "tcp",
            status="FAIL",
            baseline=BaselineRef(revision=1, p50_floor=20_001),
        )
        tls = result("tcp-tls")
        campaign = BenchmarkCampaign(
            campaign_id=sqlite.campaign_id,
            host_label=sqlite.host_label,
            os="macos",
            phase="ao2",
            started_at=NOW,
            completed_at=NOW,
            source_revision=sqlite.source_revision,
            results=(sqlite, uds, tcp, tls),
            status="FAIL",
        )
        self.assertEqual(campaign.status, "FAIL")

    def test_mixed_fail_and_incomplete_campaign_uses_incomplete_precedence(self) -> None:
        """Campaign construction cannot diverge from the canonical policy."""
        sqlite = result("sqlite")
        uds = result(
            "uds",
            status="FAIL",
            baseline=BaselineRef(revision=1, p50_floor=20_001),
        )
        tcp = result(
            "tcp",
            status="INCOMPLETE",
            incomplete_reason="restart evidence missing",
            durability_after_restart=None,
        )
        tls = result("tcp-tls")
        campaign = BenchmarkCampaign(
            campaign_id=sqlite.campaign_id,
            host_label=sqlite.host_label,
            os="macos",
            phase="ao2",
            started_at=NOW,
            completed_at=NOW,
            source_revision=sqlite.source_revision,
            results=(sqlite, uds, tcp, tls),
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

    def test_reviewed_baseline_exception_allows_a_provenanced_lower_floor(self) -> None:
        old = BaselineSet(revision=1, entries=(BaselineEntry(
            host_label="m5-atmbench", target="tcp-tls", p50_floor=17_500,
            approved_by="quality-mgr", effective_from=NOW,
        ),))
        reviewed = BaselineSet(revision=2, entries=(BaselineEntry(
            host_label="m5-atmbench", target="tcp-tls", p50_floor=13_500,
            approved_by="Rand via D3 ratchet exception",
            effective_from=datetime(2026, 8, 26, tzinfo=timezone.utc),
            rationale="The original TLS floor was mis-seeded from TCP and never achieved.",
        ),))
        require_non_decreasing_baselines(old, reviewed)
        self.assertEqual(reviewed.entry_for("m5-atmbench", "tcp-tls").p50_floor, 13_500)

    def test_unapproved_lower_floor_cannot_claim_reviewed_provenance(self) -> None:
        old = BaselineSet(revision=1, entries=(BaselineEntry(
            host_label="m5-atmbench", target="tcp", p50_floor=17_500,
            approved_by="quality-mgr", effective_from=NOW,
        ),))
        unapproved = BaselineSet(revision=2, entries=(BaselineEntry(
            host_label="m5-atmbench", target="tcp", p50_floor=17_000,
            approved_by="quality-mgr", effective_from=datetime(2026, 8, 26, tzinfo=timezone.utc),
            rationale="No reviewed exception.",
        ),))
        with self.assertRaisesRegex(ValueError, "reviewed D3 exception"):
            require_non_decreasing_baselines(old, unapproved)

    def test_reviewed_m5_atmbench_revision_loads_with_immutable_result_snapshots(self) -> None:
        baselines = load_baselines(
            ROOT / "site/reports/send-message-benchmark/baselines.json"
        )
        self.assertEqual(baselines.revision, 5)
        self.assertEqual(baselines.entry_for("m5-atmbench", "tcp").p50_floor, 17_000)
        tls = baselines.entry_for("m5-atmbench", "tcp-tls")
        self.assertEqual(tls.p50_floor, 13_500)
        self.assertEqual(tls.approved_by, "Rand via D3 ratchet exception")
        self.assertIn("mis-seeded", tls.rationale or "")
        windows_sqlite = baselines.entry_for("windows-x64-01", "sqlite")
        self.assertEqual(windows_sqlite.p50_floor, 16_035.345045815106)
        self.assertIn("pending quality review", windows_sqlite.approved_by)

    def test_campaign_and_target_ids_are_utc_safe_and_shared(self) -> None:
        identifier = campaign_id(started_at=NOW, host_label="rand-m5")
        self.assertEqual(identifier, "20260824T000000Z-rand-m5")
        self.assertEqual(
            artifact_id(campaign_id=identifier, target="tcp-tls"),
            "20260824T000000Z-rand-m5-tcp-tls",
        )

    def test_read_only_v3_model_drops_retired_acceptance_metadata(self) -> None:
        legacy = {
            "generated_at": "2026-08-24T00:00:00Z",
            "host_label": "rand-m5",
            "transport": "tcp",
            "frames_per_connection": 8,
            "messages_per_connection": 8,
            "requested_messages_per_sample": 10,
            "minimum_sample_count": 1,
            "sample_count": 0,
            "target_duration_s": 20,
            "run_duration_s": 0,
            "passed": False,
            "failure": "legacy setup failed",
            "comparison" + "_source_revision": "a" * 40,
            "threshold" + "s": {"passed": False},
        }
        normalized = BenchmarkSummary.model_validate(legacy).model_dump()
        self.assertNotIn("comparison" + "_source_revision", normalized)
        self.assertNotIn("threshold" + "s", normalized)

    def test_empty_historical_record_round_trips_through_the_normative_model(self) -> None:
        """AO2.11 can render before AO2.12 contributes any history."""
        empty = HistoricalRecord(
            schema_version=1,
            generated_from_commit="a" * 40,
            campaigns=(), ratchet=(), unattributed=(),
        )
        self.assertEqual(HistoricalRecord.model_validate_json(empty.model_dump_json()), empty)

    def test_each_historical_contract_model_documents_its_ao212_ownership(self) -> None:
        """Keep the migration boundary visible at every public model class."""
        for model in (
            RatchetPoint,
            HistoricalResultEntry,
            HistoricalCampaignEntry,
            UnattributedEntry,
            HistoricalRecord,
        ):
            self.assertIsNotNone(model.__doc__)
            self.assertIn("AO2.12", model.__doc__)

    def test_historical_record_rejects_non_monotonic_ratchet_and_wrong_campaign_result(self) -> None:
        tcp = result()
        campaign = BenchmarkCampaign(
            campaign_id=tcp.campaign_id, host_label=tcp.host_label, os=tcp.os,
            phase="ao2", started_at=NOW, completed_at=NOW,
            source_revision=tcp.source_revision, results=(tcp,), status="INCOMPLETE",
        )
        entry = HistoricalResultEntry(
            result=tcp, displayed_status="PASS", evidence_gap=None, source_files=("source.json",),
        )
        historical_campaign = HistoricalCampaignEntry(
            campaign=campaign, final_best=True, results=(entry,),
        )
        with self.assertRaisesRegex(ValidationError, "non-decreasing"):
            HistoricalRecord(
                schema_version=1, generated_from_commit="a" * 40,
                campaigns=(historical_campaign,), unattributed=(UnattributedEntry(source_file="orphan.json", reason="no group"),),
                ratchet=(
                    RatchetPoint(host_label="rand-m5", target="tcp", effective_from=NOW, p50_floor=20_000, source_campaign_id=tcp.campaign_id),
                    RatchetPoint(host_label="rand-m5", target="tcp", effective_from=NOW.replace(microsecond=1), p50_floor=19_999, source_campaign_id=tcp.campaign_id),
                ),
            )
        mismatched = result("tcp-tls")
        with self.assertRaisesRegex(ValidationError, "preserve campaign results"):
            HistoricalCampaignEntry(
                campaign=campaign,
                final_best=True,
                results=(HistoricalResultEntry(result=mismatched, displayed_status="PASS", evidence_gap=None, source_files=("wrong.json",)),),
            )


if __name__ == "__main__":
    unittest.main()

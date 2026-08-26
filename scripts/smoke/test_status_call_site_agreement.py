"""AO2.13: prove every classify_status() call site agrees, permanently.

benchmark_policy.classify_status() is the sole PASS/FAIL/INCOMPLETE decision
owner for both target-level and campaign-level roll-up.  This module drives
the same inputs through three independent call sites:

  (a) classify_status() itself,
  (b) the BenchmarkRunResult/BenchmarkCampaign pydantic model validators, and
  (c) migrate_benchmark_history's result-construction path,

and asserts all three agree.  If any call site regrows local status logic
instead of delegating to classify_status(), this fails in CI.

A companion test class scans scripts/smoke/*.py for the retired local-policy
identifiers this refactor removed, so a regrowth attempt fails even before it
reaches a call site covered above.
"""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_policy import classify_status
from scripts.smoke.benchmark_schema import (
    BaselineRef,
    BenchmarkCampaign,
    BenchmarkRunResult,
    BenchmarkSummary,
    DurabilityAfterRestart,
    required_targets,
)

SCRIPT = ROOT / "scripts/smoke/migrate_benchmark_history.py"
spec = importlib.util.spec_from_file_location("migrate_benchmark_history_agreement", SCRIPT)
assert spec and spec.loader
MIGRATE = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = MIGRATE
spec.loader.exec_module(MIGRATE)


NOW = "2026-08-24T00:00:00Z"


def distribution(value: float) -> dict:
    return {"min": value, "p50": value, "p95": value, "p99": value, "max": value}


def target_metrics(*, target: str, requested: int, admitted: int, p50: float = 20_000.0) -> dict:
    """A minimal metrics payload shaped for ``target``; classify_status itself
    never looks at target -- only at lifecycle/counts/rate/floor."""
    data = {
        "interval_count": 1,
        "passed_interval_count": 1,
        "accepted_count": admitted,
        "requested_count": requested,
        "response_count": admitted,
        "admissions_per_second": distribution(p50),
        "time_to_send_1k_s": distribution(0.0),
        "interval_latency_ms": distribution(0.0),
    }
    if target != "sqlite":
        data.update({
            "connection_count": 1,
            "application_wire_bytes": {"request": 1, "response": 1, "total": 2},
            "request_frames_per_second": distribution(p50),
            "connections_per_second": distribution(1.0),
            "application_wire_bytes_per_second": distribution(1.0),
        })
    return data


def legacy_summary(
    *,
    requested: int,
    admitted: int,
    durable: int | None,
    p50: float,
    host_label: str = "rand-bench",
    campaign_id: str = "campaign-agree",
    generated_at: str = NOW,
) -> BenchmarkSummary:
    """A v3 sqlite legacy summary shaped like migrate_benchmark_history's real input."""
    payload = {
        "schema_version": 3,
        "artifact_kind": "send_message_benchmark_summary",
        "generated_at": generated_at,
        "campaign_id": campaign_id,
        "host_label": host_label,
        "host_os": "macos",
        "transport": "sqlite",
        "benchmark_target": "sqlite",
        "frames_per_connection": 1,
        "messages_per_connection": 1,
        "requested_messages_per_sample": max(requested, 1),
        "minimum_sample_count": 1,
        "sample_count": 1,
        "target_duration_s": 1.0,
        "run_duration_s": 1.0,
        "source_revision": "a" * 40,
        "metrics": target_metrics(target="sqlite", requested=requested, admitted=admitted, p50=p50),
        "passed": durable == admitted == requested,
        "failure": None if durable == admitted == requested else "did not durably admit every message",
    }
    if durable is not None:
        payload["durability_after_restart"] = {
            "method": "isolated_sqlite_exact_count_after_restart",
            "expected_accepted_count": admitted,
            "observed_mailbox_count": durable,
            "passed": durable == admitted,
        }
    return BenchmarkSummary.model_validate(payload)


def write_target_source(
    directory: Path, *, target: str, status: str, host_label: str, campaign_id: str,
    generated_at: str, index: int,
) -> None:
    """Write one legacy-shaped source file achieving ``status`` for ``target``."""
    transport = {"sqlite": "sqlite", "uds": "uds", "tcp": "tcp", "tcp-tls": "tcp"}[target]
    peer_wire_security = {"tcp-tls": "mutual-tls", "tcp": "plaintext-test"}.get(target)
    requested = admitted = 10
    durable = 10
    include_durability = True
    if status == "FAIL":
        durable = 9
    elif status == "INCOMPLETE":
        include_durability = False
    payload = {
        "schema_version": 3,
        "artifact_kind": "send_message_benchmark_summary",
        "generated_at": generated_at,
        "campaign_id": campaign_id,
        "host_label": host_label,
        "host_os": "macos",
        "transport": transport,
        "peer_wire_security": peer_wire_security,
        "benchmark_target": target,
        "frames_per_connection": 1,
        "messages_per_connection": 1,
        "requested_messages_per_sample": requested,
        "minimum_sample_count": 1,
        "sample_count": 1,
        "target_duration_s": 1.0,
        "run_duration_s": 1.0,
        "source_revision": "a" * 40,
        "metrics": target_metrics(target=target, requested=requested, admitted=admitted),
        "passed": status == "PASS",
        "failure": None if status == "PASS" else "did not durably admit every message",
    }
    if include_durability:
        payload["durability_after_restart"] = {
            "method": "isolated_sqlite_exact_count_after_restart",
            "expected_accepted_count": admitted,
            "observed_mailbox_count": durable,
            "passed": durable == admitted,
        }
    (directory / f"source-{target}-{index}.json").write_text(json.dumps(payload), encoding="utf-8")


def run_result_for(*, target: str, status: str, campaign_id: str, host_label: str) -> BenchmarkRunResult:
    """A BenchmarkRunResult achieving ``status`` for ``target``, floor-independent."""
    requested = admitted = 10
    durable = 10
    durability: DurabilityAfterRestart | None
    incomplete_reason = None
    if status == "FAIL":
        durable = 9
        durability = DurabilityAfterRestart(
            expected_accepted_count=admitted, observed_mailbox_count=durable, passed=False,
        )
    elif status == "INCOMPLETE":
        durability = None
        incomplete_reason = "durability missing"
    else:
        durability = DurabilityAfterRestart(
            expected_accepted_count=admitted, observed_mailbox_count=durable, passed=True,
        )
    return BenchmarkRunResult(
        campaign_id=campaign_id, host_label=host_label, os="macos", target=target, status=status,
        incomplete_reason=incomplete_reason, generated_at=NOW, source_revision="a" * 40, binary_hashes={},
        frames_per_connection=0 if target == "sqlite" else 8,
        messages_requested=requested, messages_admitted=admitted,
        messages_durable=0 if status == "INCOMPLETE" else durable,
        metrics=target_metrics(target=target, requested=requested, admitted=admitted),
        baseline=BaselineRef(revision=1, p50_floor=100.0),
        durability_after_restart=durability,
    )


class TargetLevelAgreementTests(unittest.TestCase):
    """PASS / FAIL(non-durable) / FAIL(below-floor) / INCOMPLETE agreement."""

    CASES = {
        "pass": dict(requested=10, admitted=10, durable=10, p50=20_000.0, floor=17_500.0),
        "fail_non_durable": dict(requested=10, admitted=10, durable=9, p50=20_000.0, floor=17_500.0),
        "fail_below_floor": dict(requested=10, admitted=10, durable=10, p50=17_000.0, floor=17_500.0),
        "incomplete": dict(requested=10, admitted=10, durable=None, p50=20_000.0, floor=17_500.0),
    }

    def classify(self, case: dict) -> str:
        return classify_status(
            lifecycle_complete=case["durable"] is not None,
            messages_requested=case["requested"],
            messages_admitted=case["admitted"],
            messages_durable=0 if case["durable"] is None else case["durable"],
            p50_admissions_per_second=None if case["durable"] is None else case["p50"],
            baseline_p50_floor=case["floor"],
        )

    def test_run_result_model_validation_agrees(self) -> None:
        for name, case in self.CASES.items():
            with self.subTest(name):
                expected = self.classify(case)
                durability = None
                if case["durable"] is not None:
                    durability = DurabilityAfterRestart(
                        expected_accepted_count=case["admitted"],
                        observed_mailbox_count=case["durable"],
                        passed=case["durable"] == case["admitted"],
                    )
                result = BenchmarkRunResult(
                    campaign_id="20260824T000000Z-rand-bench",
                    host_label="rand-bench",
                    os="macos",
                    target="sqlite",
                    status=expected,
                    incomplete_reason=None if expected != "INCOMPLETE" else "durability missing",
                    generated_at=NOW,
                    source_revision="a" * 40,
                    binary_hashes={},
                    frames_per_connection=0,
                    messages_requested=case["requested"],
                    messages_admitted=case["admitted"],
                    messages_durable=0 if case["durable"] is None else case["durable"],
                    metrics=target_metrics(
                        target="sqlite", requested=case["requested"], admitted=case["admitted"], p50=case["p50"],
                    ),
                    baseline=BaselineRef(revision=1, p50_floor=case["floor"]),
                    durability_after_restart=durability,
                )
                self.assertEqual(result.status, expected)

    def test_migrate_result_construction_path_agrees(self) -> None:
        for name, case in self.CASES.items():
            with self.subTest(name):
                expected = self.classify(case)
                summary = legacy_summary(
                    requested=case["requested"], admitted=case["admitted"],
                    durable=case["durable"], p50=case["p50"],
                )
                result, _gap = MIGRATE.result_from_summary(
                    summary, Path("agreement-fixture.json"), "campaign-agree", case["floor"],
                )
                self.assertEqual(result.status, expected)


class CampaignRollUpAgreementTests(unittest.TestCase):
    """All-PASS / FAIL-present / INCOMPLETE-present / missing-target agreement."""

    CASES = {
        "all_pass": {"sqlite": "PASS", "uds": "PASS", "tcp": "PASS", "tcp-tls": "PASS"},
        "fail_present": {"sqlite": "PASS", "uds": "PASS", "tcp": "FAIL", "tcp-tls": "PASS"},
        "incomplete_present": {"sqlite": "PASS", "uds": "INCOMPLETE", "tcp": "PASS", "tcp-tls": "PASS"},
        "missing_target": {"sqlite": "PASS", "tcp": "PASS", "tcp-tls": "PASS"},
    }

    def test_campaign_model_and_migrate_roll_up_agree(self) -> None:
        for name, targets in self.CASES.items():
            with self.subTest(name):
                expected = classify_status(
                    required_targets=required_targets("macos"),
                    observed_targets=set(targets),
                    target_statuses=tuple(targets.values()),
                )

                # (b) BenchmarkCampaign model validation.
                campaign_id = f"20260824T000000Z-camp-{name}"
                results = tuple(
                    run_result_for(target=target, status=status, campaign_id=campaign_id, host_label="rand-bench")
                    for target, status in targets.items()
                )
                campaign = BenchmarkCampaign(
                    campaign_id=campaign_id, host_label="rand-bench", os="macos", phase="ao2",
                    started_at=NOW, completed_at=NOW, source_revision="a" * 40,
                    results=results, status=expected,
                )
                self.assertEqual(campaign.status, expected)

                # (c) migrate_benchmark_history's own campaign roll-up.
                with tempfile.TemporaryDirectory() as temp:
                    reports = Path(temp)
                    for index, (target, status) in enumerate(targets.items()):
                        write_target_source(
                            reports, target=target, status=status, host_label="rand-bench",
                            campaign_id=campaign_id, generated_at=NOW, index=index,
                        )
                    (reports / "baselines.json").write_text(
                        json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8",
                    )
                    record, _audit = MIGRATE.migrated_record(reports, "0" * 40)
                migrated = next(entry for entry in record.campaigns if entry.campaign.campaign_id == campaign_id)
                self.assertEqual(migrated.campaign.status, expected)


BANNED_IDENTIFIERS = (
    "evaluate_profile_thresholds",
    "comparison_ratio",
    "comparison_strict",
    "comparison_required",
    "TARGET_MSG_PER_SECOND",
    "tcp+tls",
)


class RetiredLocalPolicyIdentifiersStayGoneTests(unittest.TestCase):
    """AO2.13 removed these identifiers; a regrowth attempt must fail CI.

    Deterministic file-content grep, no subprocess and no timing dependency,
    per ADR-008 (no flaky tests).
    """

    def test_no_smoke_source_reintroduces_a_banned_identifier(self) -> None:
        offenders: list[str] = []
        for path in sorted((ROOT / "scripts/smoke").glob("*.py")):
            if path == Path(__file__).resolve():
                continue
            text = path.read_text(encoding="utf-8")
            for identifier in BANNED_IDENTIFIERS:
                if identifier in text:
                    offenders.append(f"{path.name}: {identifier}")
        self.assertEqual(offenders, [], f"banned local-status-policy identifiers found: {offenders}")


if __name__ == "__main__":
    unittest.main()

"""Focused invariant tests for the mandatory four-target benchmark suite."""
from __future__ import annotations

import hashlib
from pathlib import Path
import tempfile
import unittest

from scripts.smoke import benchmark_suite as SUITE


SHA = "a" * 40
HASH = "b" * 64


def telemetry() -> SUITE.HostTelemetry:
    return SUITE.HostTelemetry(
        logical_cpu_count=8, load_average_1m=0.5, competing_process_cpu_percent=0,
        benchmark_process_cpu_percent=90, available_memory_bytes=1, free_disk_bytes=1,
        kernel_release="test", power_mode="test", sample_interval_seconds=1,
        observation_duration_seconds=10,
        competing_cpu_at_or_above_20_percent_seconds=0,
        load_above_125_percent_cpu_seconds=0,
    )


def result(target: str, rate: float = 50_000.0) -> SUITE.TargetResult:
    return SUITE.TargetResult(
        target=target, median_msg_per_second=rate, p95_msg_per_second=rate,
        p99_msg_per_second=rate, requested=10_000, accepted=10_000, errors=0,
        raw_artifact=f"raw/{target}.json", raw_artifact_sha256=HASH,
    )


def intent(sequence: int, *, harness_revision: str = SHA) -> SUITE.SuiteIntent:
    return SUITE.SuiteIntent(
        sequence=sequence, suite_id=f"suite-20260822T080000Z-{sequence:016x}",
        started_at="2026-08-22T08:00:00Z", candidate_revision=SHA,
        production_revision=SHA, harness_revision=harness_revision,
    )


def attempt(
    sequence: int, *, rate: float = 50_000.0, restored: bool = True,
    errors: int = 0, harness_revision: str = SHA,
) -> SUITE.CompleteSuiteAttempt:
    return SUITE.CompleteSuiteAttempt(
        sequence=sequence, suite_id=f"suite-20260822T080000Z-{sequence:016x}",
        started_at="2026-08-22T08:00:00Z", completed_at="2026-08-22T08:01:00Z",
        candidate_revision=SHA, production_revision=SHA, harness_revision=harness_revision,
        results=[
            result(target, rate).model_copy(update={"accepted": 10_000 - errors, "errors": errors})
            for target in SUITE.REQUIRED_TARGETS
        ],
        snapshot_id="snapshot-20260822T080000Z-0123456789abcdef",
        restore_verified=restored, telemetry_before=telemetry(), telemetry_after=telemetry(),
        raw_artifact_sha256=HASH,
    )


def ledger(
    attempts: list[SUITE.CompleteSuiteAttempt], accepted: bool = False,
    *, intents: list[SUITE.SuiteIntent] | None = None,
    checkpoint: SUITE.SameRevisionRerunCheckpoint | None = None,
) -> SUITE.M5AttemptLedger:
    return SUITE.M5AttemptLedger(
        candidate_revision=SHA, host="rand-m5.local",
        f8=SUITE.F8Profile(request_body_sha256=HASH),
        thresholds=[
            SUITE.TargetThreshold(target=target, expected_msg_per_second=45_000, closure_floor_msg_per_second=42_750)
            for target in SUITE.REQUIRED_TARGETS
        ],
        intents=intents if intents is not None else [intent(item.sequence) for item in attempts],
        attempts=attempts, same_revision_rerun_checkpoint=checkpoint, accepted_m5=accepted,
    )


class BenchmarkSuiteTests(unittest.TestCase):
    def test_results_require_the_exact_four_target_order(self) -> None:
        with self.assertRaisesRegex(ValueError, "in order"):
            payload = attempt(1).model_dump(mode="json")
            payload["results"] = list(reversed(payload["results"]))
            SUITE.CompleteSuiteAttempt.model_validate(payload)

    def test_ledger_rejects_hand_edited_acceptance(self) -> None:
        with self.assertRaisesRegex(ValueError, "accepted_m5"):
            ledger([], accepted=True)

    def test_ledger_requires_contiguous_attempt_history(self) -> None:
        with self.assertRaisesRegex(ValueError, "contiguous"):
            ledger([attempt(1), attempt(3)])

    def test_final_three_consecutive_successes_derive_acceptance(self) -> None:
        entries = [attempt(1), attempt(2), attempt(3)]
        self.assertTrue(ledger(entries, accepted=True).accepted_m5)

    def test_low_result_or_failed_restore_blocks_acceptance(self) -> None:
        self.assertFalse(ledger([attempt(1), attempt(2), attempt(3, rate=1)], accepted=False).accepted_m5)
        self.assertFalse(ledger([attempt(1), attempt(2), attempt(3, restored=False)], accepted=False).accepted_m5)

    def test_error_free_is_an_acceptance_requirement(self) -> None:
        self.assertFalse(ledger([attempt(1), attempt(2), attempt(3, errors=1)], accepted=False).accepted_m5)

    def test_uncompleted_intent_blocks_acceptance_and_remains_visible(self) -> None:
        entries = [attempt(1), attempt(2), attempt(3)]
        self.assertFalse(
            ledger(entries, accepted=False, intents=[intent(1), intent(2), intent(3), intent(4)]).accepted_m5
        )

    def test_completion_requires_prior_intent(self) -> None:
        with self.assertRaisesRegex(ValueError, "prior intent"):
            ledger([attempt(1)], intents=[])

    def test_same_revision_recovery_after_failure_requires_checkpoint(self) -> None:
        entries = [attempt(1, rate=1), attempt(2), attempt(3), attempt(4)]
        self.assertFalse(ledger(entries, accepted=False).accepted_m5)
        checkpoint = SUITE.SameRevisionRerunCheckpoint(
            last_failed_sequence=1, first_recovery_sequence=2,
            reviewed_at="2026-08-22T08:02:00Z", rationale="host contention resolved",
        )
        self.assertTrue(ledger(entries, accepted=True, checkpoint=checkpoint).accepted_m5)

    def test_host_telemetry_requires_ten_second_observation(self) -> None:
        payload = telemetry().model_dump()
        payload["observation_duration_seconds"] = 9
        with self.assertRaisesRegex(ValueError, "greater than or equal"):
            SUITE.HostTelemetry.model_validate(payload)

    def test_new_candidate_cannot_hide_a_prior_unresolved_ledger(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = SUITE.create_m5_ledger(
                root, candidate_revision=SHA, host="rand-m5.local",
                f8=SUITE.F8Profile(request_body_sha256=HASH),
                thresholds=[
                    SUITE.TargetThreshold(target=target, expected_msg_per_second=45_000, closure_floor_msg_per_second=42_750)
                    for target in SUITE.REQUIRED_TARGETS
                ],
            )
            self.assertFalse(first.accepted_m5)
            successor = "c" * 40
            with self.assertRaisesRegex(SUITE.BenchmarkSuiteError, "lineage"):
                SUITE.create_m5_ledger(
                    root, candidate_revision=successor, host="rand-m5.local",
                    f8=first.f8, thresholds=first.thresholds,
                )
            lineage = SUITE.CandidateLineage(
                prior_candidate_revision=SHA,
                prior_ledger_sha256=SUITE.raw_file_sha256(SUITE.suite_ledger_path(root, SHA)),
                reviewed_at="2026-08-22T08:02:00Z",
                disposition="reviewed_failed_or_incomplete", rationale="writer fix follows RCA",
            )
            created = SUITE.create_m5_ledger(
                root, candidate_revision=successor, host="rand-m5.local",
                f8=first.f8, thresholds=first.thresholds, lineage=lineage,
            )
            self.assertEqual(created.lineage, lineage)

    def test_windows_host_facts_reject_elevation_and_virtualization(self) -> None:
        facts = {
            "native_os": "windows", "cpu_model": "test", "power_plan": "balanced",
            "defender_or_av_status": "enabled", "av_exclusions_present": False,
            "virtualization_detected": False, "benchmark_token_elevated": False,
            "wsl_detected": False,
        }
        self.assertEqual(SUITE.WindowsHostFacts.model_validate(facts).native_os, "windows")
        facts["benchmark_token_elevated"] = True
        with self.assertRaisesRegex(ValueError, "standard token"):
            SUITE.WindowsHostFacts.model_validate(facts)

    def test_windows_artifact_path_is_candidate_specific(self) -> None:
        self.assertEqual(
            SUITE.windows_artifact_path(Path("docs/plans/phase-ao2/artifacts"), SHA).name,
            f"ao2-8-fastpc4-suite-{SHA}.json",
        )

    def test_append_persists_derived_ledger_at_its_only_candidate_path(self) -> None:
        initial = ledger([])
        with tempfile.TemporaryDirectory() as directory:
            path = SUITE.suite_ledger_path(Path(directory), SHA)
            intended = SUITE.append_suite_intent(path, initial, intent(1))
            updated = SUITE.append_completed_attempt(path, intended, attempt(1))
            self.assertTrue(path.is_file())
        self.assertEqual(len(updated.attempts), 1)
        self.assertFalse(updated.accepted_m5)

    def test_raw_file_hash_is_content_addressed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "raw.json"
            path.write_bytes(b"raw evidence")
            self.assertEqual(SUITE.raw_file_sha256(path), hashlib.sha256(b"raw evidence").hexdigest())


if __name__ == "__main__":
    unittest.main()

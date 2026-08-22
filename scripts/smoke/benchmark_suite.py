"""Immutable contract for one complete AO2.7 benchmark-matrix attempt.

The ordinary benchmark is a four-target measurement, not four independently
acceptable commands.  This module makes that invariant explicit so evidence
cannot accidentally turn a partial or selectively re-run set into a pass.
"""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import tempfile
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


SUITE_SCHEMA_VERSION = 2
F8_WORKLOAD_ID = "f8-v1"
REQUIRED_TARGETS = ("sqlite", "uds", "tcp", "tcp-tls")
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
SAFE_RELATIVE_ARTIFACT = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,255}$")


class BenchmarkSuiteError(ValueError):
    """A suite ledger is malformed or cannot be safely published."""


class F8Profile(BaseModel):
    """The frozen acceptance workload shared by all four targets."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    workload_id: Literal["f8-v1"] = F8_WORKLOAD_ID
    frames_per_connection: Literal[8] = 8
    requests_per_interval: Literal[1000] = 1000
    minimum_interval_count: Literal[10] = 10
    minimum_timed_seconds: Literal[20] = 20
    worker_limit: Literal[64] = 64
    max_in_flight: Literal[8] = 8
    request_body_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")


class TargetThreshold(BaseModel):
    """One target's expected throughput and tolerant closure floor."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    expected_msg_per_second: float = Field(gt=0)
    closure_floor_msg_per_second: float = Field(gt=0)

    @model_validator(mode="after")
    def floor_is_not_above_expectation(self) -> "TargetThreshold":
        if self.closure_floor_msg_per_second > self.expected_msg_per_second:
            raise ValueError("closure floor cannot exceed expected throughput")
        return self


class TargetResult(BaseModel):
    """One required target's compact measurement and immutable raw reference."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    median_msg_per_second: float = Field(ge=0)
    p95_msg_per_second: float = Field(ge=0)
    p99_msg_per_second: float = Field(ge=0)
    requested: int = Field(ge=0)
    accepted: int = Field(ge=0)
    errors: int = Field(ge=0)
    raw_artifact: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,255}$")
    raw_artifact_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")

    @model_validator(mode="after")
    def counts_and_rates_are_consistent(self) -> "TargetResult":
        if self.accepted > self.requested:
            raise ValueError("accepted cannot exceed requested")
        if self.errors != self.requested - self.accepted:
            raise ValueError("errors must equal requested minus accepted")
        if not SAFE_RELATIVE_ARTIFACT.fullmatch(self.raw_artifact):
            raise ValueError("raw artifact path must be a safe relative path")
        return self


class HostTelemetry(BaseModel):
    """Duration-bearing host facts collected around one suite.

    The counts are pre-aggregated from samples at ``sample_interval_seconds``.
    They make the ten-second contention/materiality policy mechanically
    checkable instead of relying on two instantaneous observations.
    """

    model_config = ConfigDict(extra="forbid", frozen=True)

    logical_cpu_count: int = Field(gt=0)
    load_average_1m: float = Field(ge=0)
    competing_process_cpu_percent: float = Field(ge=0)
    benchmark_process_cpu_percent: float = Field(ge=0)
    available_memory_bytes: int = Field(ge=0)
    free_disk_bytes: int = Field(ge=0)
    kernel_release: str = Field(min_length=1)
    power_mode: str = Field(min_length=1)
    sample_interval_seconds: float = Field(gt=0)
    observation_duration_seconds: float = Field(ge=10)
    competing_cpu_at_or_above_20_percent_seconds: float = Field(ge=0)
    load_above_125_percent_cpu_seconds: float = Field(ge=0)

    @model_validator(mode="after")
    def materiality_durations_fit_the_observation(self) -> "HostTelemetry":
        if self.competing_cpu_at_or_above_20_percent_seconds > self.observation_duration_seconds:
            raise ValueError("competing CPU duration cannot exceed observation duration")
        if self.load_above_125_percent_cpu_seconds > self.observation_duration_seconds:
            raise ValueError("load duration cannot exceed observation duration")
        return self

    @property
    def material_contention(self) -> bool:
        """Whether the documented M5 host-noise bar was actually crossed."""
        return (
            self.competing_cpu_at_or_above_20_percent_seconds >= 10
            or self.load_above_125_percent_cpu_seconds > 0
        )


class SuiteIntent(BaseModel):
    """Append-only intent written before a suite may touch benchmark state.

    A crashed process therefore leaves a durable, visible unmatched intent.
    Completion is a second append, not an update that can erase the intent.
    """

    model_config = ConfigDict(extra="forbid", frozen=True)

    sequence: int = Field(gt=0)
    suite_id: str = Field(pattern=r"^suite-[0-9]{8}T[0-9]{6}Z-[a-f0-9]{16}$")
    started_at: str = Field(min_length=1)
    candidate_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    production_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    harness_revision: str = Field(pattern=r"^[0-9a-f]{40}$")


class CandidateLineage(BaseModel):
    """Explicit review record when a fresh candidate supersedes another ledger."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    prior_candidate_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    prior_ledger_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    reviewed_at: str = Field(min_length=1)
    disposition: Literal["accepted_baseline", "reviewed_failed_or_incomplete"]
    rationale: str = Field(min_length=1)


class SameRevisionRerunCheckpoint(BaseModel):
    """Makes a no-code-change recovery series visible and reviewable."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    last_failed_sequence: int = Field(gt=0)
    first_recovery_sequence: int = Field(gt=0)
    reviewed_at: str = Field(min_length=1)
    rationale: str = Field(min_length=1)

    @model_validator(mode="after")
    def checkpoint_orders_failure_before_recovery(self) -> "SameRevisionRerunCheckpoint":
        if self.first_recovery_sequence <= self.last_failed_sequence:
            raise ValueError("recovery sequence must follow the failed sequence")
        return self


class WindowsHostFacts(BaseModel):
    """Native Windows facts required before a fastpc4 result is comparable."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    native_os: Literal["windows"]
    cpu_model: str = Field(min_length=1)
    power_plan: str = Field(min_length=1)
    defender_or_av_status: str = Field(min_length=1)
    av_exclusions_present: bool
    virtualization_detected: bool
    benchmark_token_elevated: bool
    wsl_detected: bool

    @model_validator(mode="after")
    def native_standard_token_environment_is_required(self) -> "WindowsHostFacts":
        if self.av_exclusions_present:
            raise ValueError("Windows benchmark evidence must not use AV exclusions")
        if self.virtualization_detected or self.wsl_detected:
            raise ValueError("Windows benchmark evidence must be native, not virtualized or WSL")
        if self.benchmark_token_elevated:
            raise ValueError("Windows benchmark evidence must use a standard token")
        return self


class WindowsParityArtifact(BaseModel):
    """Committed fastpc4 evidence linked to one accepted M5 suite manifest."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[SUITE_SCHEMA_VERSION] = SUITE_SCHEMA_VERSION
    candidate_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    m5_ledger_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    host: Literal["fastpc4"]
    host_facts: WindowsHostFacts
    f8: F8Profile
    attempts: list[CompleteSuiteAttempt]

    @model_validator(mode="after")
    def attempts_are_complete_matrix_results(self) -> "WindowsParityArtifact":
        if not self.attempts:
            raise ValueError("Windows artifact requires at least one complete suite attempt")
        return self


class CompleteSuiteAttempt(BaseModel):
    """An append-only four-target attempt, whether it passed or failed."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    sequence: int = Field(gt=0)
    suite_id: str = Field(pattern=r"^suite-[0-9]{8}T[0-9]{6}Z-[a-f0-9]{16}$")
    started_at: str = Field(min_length=1)
    completed_at: str = Field(min_length=1)
    candidate_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    production_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    harness_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    results: list[TargetResult] = Field(min_length=4, max_length=4)
    snapshot_id: str = Field(pattern=r"^snapshot-[0-9]{8}T[0-9]{6}Z-[a-f0-9]{16}$")
    restore_verified: bool
    telemetry_before: HostTelemetry
    telemetry_after: HostTelemetry
    raw_artifact_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")

    @model_validator(mode="after")
    def has_one_ordered_result_for_every_target(self) -> "CompleteSuiteAttempt":
        if tuple(result.target for result in self.results) != REQUIRED_TARGETS:
            raise ValueError("results must contain sqlite, uds, tcp, tcp-tls in order")
        return self


class M5AttemptLedger(BaseModel):
    """Candidate-specific history; acceptance is derived, never caller selected."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[SUITE_SCHEMA_VERSION] = SUITE_SCHEMA_VERSION
    candidate_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    host: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    f8: F8Profile
    thresholds: list[TargetThreshold] = Field(min_length=4, max_length=4)
    lineage: CandidateLineage | None = None
    intents: list[SuiteIntent]
    attempts: list[CompleteSuiteAttempt]
    same_revision_rerun_checkpoint: SameRevisionRerunCheckpoint | None = None
    accepted_m5: bool

    @model_validator(mode="after")
    def acceptance_is_derived_and_history_is_contiguous(self) -> "M5AttemptLedger":
        if tuple(item.target for item in self.thresholds) != REQUIRED_TARGETS:
            raise ValueError("thresholds must contain sqlite, uds, tcp, tcp-tls in order")
        if [intent.sequence for intent in self.intents] != list(range(1, len(self.intents) + 1)):
            raise ValueError("intent sequences must start at one and be contiguous")
        intent_by_sequence = {intent.sequence: intent for intent in self.intents}
        attempt_sequences = [attempt.sequence for attempt in self.attempts]
        if len(set(attempt_sequences)) != len(attempt_sequences):
            raise ValueError("a suite intent can have only one completion")
        if attempt_sequences != sorted(attempt_sequences):
            raise ValueError("completed attempt sequences must be ordered")
        for attempt in self.attempts:
            intent = intent_by_sequence.get(attempt.sequence)
            if intent is None:
                raise ValueError("completed attempt must reference a prior intent")
            if attempt.suite_id != intent.suite_id:
                raise ValueError("completed attempt suite id must match its intent")
            if (
                attempt.candidate_revision != self.candidate_revision
                or intent.candidate_revision != self.candidate_revision
                or attempt.production_revision != intent.production_revision
                or attempt.harness_revision != intent.harness_revision
            ):
                raise ValueError("attempt revision fields must match its intent and ledger")
        if self.same_revision_rerun_checkpoint is not None:
            sequences = set(attempt_sequences)
            if (
                self.same_revision_rerun_checkpoint.last_failed_sequence not in sequences
                or self.same_revision_rerun_checkpoint.first_recovery_sequence not in sequences
            ):
                raise ValueError("rerun checkpoint must reference completed attempts")
        if self.accepted_m5 != derive_accepted_m5(
            self.attempts,
            self.thresholds,
            self.intents,
            self.same_revision_rerun_checkpoint,
        ):
            raise ValueError("accepted_m5 must match the derived final-three result")
        return self


def derive_accepted_m5(
    attempts: list[CompleteSuiteAttempt], thresholds: list[TargetThreshold],
    intents: list[SuiteIntent] | None = None,
    checkpoint: SameRevisionRerunCheckpoint | None = None,
) -> bool:
    """Accept only an error-free terminal three-suite series.

    All started intents must be complete; a failed run followed by three later
    no-code-change successes needs an explicit checkpoint rather than a lucky
    unreviewed rerun.
    """
    if len(attempts) < 3:
        return False
    if intents is not None and len(intents) != len(attempts):
        return False
    floors = {item.target: item.closure_floor_msg_per_second for item in thresholds}
    final = attempts[-3:]
    successful_final = all(
        attempt.restore_verified
        and all(
            result.errors == 0
            and result.accepted == result.requested
            and result.median_msg_per_second >= floors[result.target]
            for result in attempt.results
        )
        for attempt in final
    )
    if not successful_final:
        return False
    previous_failures = [
        attempt for attempt in attempts[:-3]
        if not _attempt_meets_thresholds(attempt, floors)
    ]
    if not previous_failures:
        return True
    last_failure = previous_failures[-1]
    return checkpoint is not None and (
        checkpoint.last_failed_sequence == last_failure.sequence
        and checkpoint.first_recovery_sequence == final[0].sequence
    )


def _attempt_meets_thresholds(attempt: CompleteSuiteAttempt, floors: dict[str, float]) -> bool:
    return attempt.restore_verified and all(
        result.errors == 0
        and result.accepted == result.requested
        and result.median_msg_per_second >= floors[result.target]
        for result in attempt.results
    )


def suite_ledger_path(root: Path, candidate_revision: str) -> Path:
    """Return the only accepted committed path for one candidate's M5 history."""
    if not GIT_REVISION.fullmatch(candidate_revision):
        raise BenchmarkSuiteError("candidate revision must be a full lowercase Git SHA")
    return root / f"ao2-7-m5-suite-{candidate_revision}.json"


def windows_artifact_path(root: Path, candidate_revision: str) -> Path:
    """Return the sole committed path for fastpc4 evidence for one candidate."""
    if not GIT_REVISION.fullmatch(candidate_revision):
        raise BenchmarkSuiteError("candidate revision must be a full lowercase Git SHA")
    return root / f"ao2-8-fastpc4-suite-{candidate_revision}.json"


def create_m5_ledger(
    root: Path,
    *,
    candidate_revision: str,
    host: str,
    f8: F8Profile,
    thresholds: list[TargetThreshold],
    lineage: CandidateLineage | None = None,
) -> M5AttemptLedger:
    """Create one candidate ledger without silently discarding predecessor history.

    Any pre-existing candidate ledger in ``root`` makes explicit lineage
    mandatory.  Its exact content hash is recorded, so a no-op candidate
    cannot make an unresolved earlier series disappear from review.
    """
    path = suite_ledger_path(root, candidate_revision)
    if path.exists():
        raise BenchmarkSuiteError("candidate ledger already exists; load and append it instead")
    predecessors = sorted(root.glob("ao2-7-m5-suite-*.json"))
    if predecessors and lineage is None:
        raise BenchmarkSuiteError("new candidate requires reviewed prior-candidate lineage")
    if lineage is not None:
        prior_path = suite_ledger_path(root, lineage.prior_candidate_revision)
        if not prior_path.is_file():
            raise BenchmarkSuiteError("lineage prior candidate ledger does not exist")
        if raw_file_sha256(prior_path) != lineage.prior_ledger_sha256:
            raise BenchmarkSuiteError("lineage prior ledger hash does not match its artifact")
        prior = M5AttemptLedger.model_validate_json(prior_path.read_text(encoding="utf-8"))
        if not prior.accepted_m5 and lineage.disposition != "reviewed_failed_or_incomplete":
            raise BenchmarkSuiteError("unresolved prior candidate requires explicit review disposition")
    elif predecessors:
        raise AssertionError("predecessor lineage should have been required")
    ledger = M5AttemptLedger(
        candidate_revision=candidate_revision,
        host=host,
        f8=f8,
        thresholds=thresholds,
        lineage=lineage,
        intents=[],
        attempts=[],
        accepted_m5=False,
    )
    _atomic_json_write(path, ledger.model_dump(mode="json"))
    return ledger


def append_suite_intent(path: Path, ledger: M5AttemptLedger, intent: SuiteIntent) -> M5AttemptLedger:
    """Persist the intent before a suite executes, then return its ledger."""
    if path != suite_ledger_path(path.parent, ledger.candidate_revision):
        raise BenchmarkSuiteError("ledger path does not match its candidate revision")
    if intent.sequence != len(ledger.intents) + 1:
        raise BenchmarkSuiteError("new suite intent sequence must append contiguously")
    if intent.candidate_revision != ledger.candidate_revision:
        raise BenchmarkSuiteError("suite intent candidate revision must match its ledger")
    payload = ledger.model_dump(mode="json")
    payload["intents"] = [*payload["intents"], intent.model_dump(mode="json")]
    payload["accepted_m5"] = derive_accepted_m5(
        ledger.attempts, ledger.thresholds, [*ledger.intents, intent], ledger.same_revision_rerun_checkpoint,
    )
    updated = M5AttemptLedger.model_validate(payload)
    _atomic_json_write(path, updated.model_dump(mode="json"))
    return updated


def append_completed_attempt(path: Path, ledger: M5AttemptLedger, attempt: CompleteSuiteAttempt) -> M5AttemptLedger:
    """Append a completion only for a previously durable suite intent."""
    if path != suite_ledger_path(path.parent, ledger.candidate_revision):
        raise BenchmarkSuiteError("ledger path does not match its candidate revision")
    if attempt.sequence not in {intent.sequence for intent in ledger.intents}:
        raise BenchmarkSuiteError("completed attempt requires a prior durable suite intent")
    payload = ledger.model_dump(mode="json")
    payload["attempts"] = [*payload["attempts"], attempt.model_dump(mode="json")]
    completed = [*ledger.attempts, attempt]
    payload["accepted_m5"] = derive_accepted_m5(
        completed, ledger.thresholds, ledger.intents, ledger.same_revision_rerun_checkpoint,
    )
    updated = M5AttemptLedger.model_validate(payload)
    _atomic_json_write(path, updated.model_dump(mode="json"))
    return updated


def raw_file_sha256(path: Path) -> str:
    """Hash raw evidence before inserting its immutable reference into a ledger."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _atomic_json_write(path: Path, payload: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", dir=path.parent, prefix=f".{path.name}.", suffix=".tmp", delete=False, encoding="utf-8",
    ) as handle:
        temporary = Path(handle.name)
        json.dump(payload, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)

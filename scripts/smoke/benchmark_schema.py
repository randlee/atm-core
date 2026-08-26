"""Versioned, public benchmark-summary schema shared by runner and migration."""
from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
import re
from typing import Any, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field, ValidationError, field_validator, model_validator

from scripts.public_redaction import public_string
from scripts.smoke.benchmark_policy import classify_status


SUMMARY_SCHEMA_VERSION = 4
LEGACY_SUMMARY_SCHEMA_VERSION = 3
SUPPORTED_TRANSPORTS = frozenset({"sqlite", "uds", "tcp"})
SUPPORTED_FRAMES = frozenset({1, 2, 4, 8, 16, 64})


class BenchmarkSchemaError(ValueError):
    """A benchmark artifact cannot be represented by the public schema."""


class MetricDistribution(BaseModel):
    """A compact distribution over independently measured intervals."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    min: float = Field(ge=0)
    p50: float = Field(ge=0)
    p95: float = Field(ge=0)
    p99: Optional[float] = Field(default=None, ge=0)
    max: float = Field(ge=0)

    @model_validator(mode="after")
    def ordered(self) -> "MetricDistribution":
        if not self.min <= self.p50 <= self.p95 <= self.max:
            raise ValueError("distribution must be ordered min <= p50 <= p95 <= max")
        if self.p99 is not None and not self.p95 <= self.p99 <= self.max:
            raise ValueError("distribution p99 must be between p95 and max")
        return self


class WireBytes(BaseModel):
    """Application bytes transferred by the measured public boundary."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    request: int = Field(ge=0)
    response: int = Field(ge=0)
    total: int = Field(ge=0)

    @model_validator(mode="after")
    def totals_match(self) -> "WireBytes":
        if self.total != self.request + self.response:
            raise ValueError("total must equal request plus response")
        return self


class BenchmarkMetrics(BaseModel):
    """Lossless-for-reporting aggregate of one verbose interval trace."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    interval_count: int = Field(gt=0)
    passed_interval_count: int = Field(ge=0)
    accepted_count: int = Field(ge=0)
    requested_count: int = Field(ge=0)
    response_count: int = Field(ge=0)
    connection_count: int | None = Field(default=None, ge=0)
    application_wire_bytes: WireBytes | None = None
    admissions_per_second: MetricDistribution
    request_frames_per_second: MetricDistribution | None = None
    connections_per_second: MetricDistribution | None = None
    application_wire_bytes_per_second: MetricDistribution | None = None
    time_to_send_1k_s: MetricDistribution
    interval_latency_ms: MetricDistribution
    first_failure: Optional[str] = None

    @model_validator(mode="after")
    def passed_count_is_bounded(self) -> "BenchmarkMetrics":
        if self.passed_interval_count > self.interval_count:
            raise ValueError("passed_interval_count cannot exceed interval_count")
        return self


class DurabilityAfterRestart(BaseModel):
    """Exact isolated-store count recorded after the owned daemon restarts."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    method: str = "isolated_sqlite_exact_count_after_restart"
    expected_accepted_count: int = Field(ge=0)
    observed_mailbox_count: int = Field(ge=0)
    passed: bool


class DirectSQLiteMessageWrite(BaseModel):
    """One direct Tokio admission measurement against the shared SQLite writer.

    This is intentionally separate from the public HTTP transport profile: it
    is the durable-message-write ceiling for the exact async store used by the
    runtime, and makes a transport regression distinguishable from a storage
    regression in the published evidence.
    """

    model_config = ConfigDict(extra="forbid", frozen=True)

    kind: Literal["async_storage_admission", "canonical_core_write"] = "async_storage_admission"
    requested_count: int = Field(gt=0)
    accepted_count: int = Field(ge=0)
    worker_count: int = Field(gt=0)
    elapsed_seconds: float = Field(gt=0)
    admissions_per_second: float = Field(gt=0)

    @model_validator(mode="after")
    def accepted_count_is_bounded(self) -> "DirectSQLiteMessageWrite":
        if self.accepted_count > self.requested_count:
            raise ValueError("accepted_count cannot exceed requested_count")
        return self


class BaselineEntry(BaseModel):
    """One quality-reviewed per-host/per-target v4 acceptance floor."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    host_label: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    p50_floor: float = Field(ge=0)
    approved_by: str = Field(min_length=1)
    effective_from: datetime
    rationale: str | None = Field(default=None, min_length=1)

    @field_validator("effective_from")
    @classmethod
    def utc_effective_from(cls, value: datetime) -> datetime:
        return require_utc(value, "effective_from")


class BaselineSet(BaseModel):
    """The one reviewed source of benchmark acceptance floors."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[1] = 1
    revision: int = Field(ge=1)
    entries: tuple[BaselineEntry, ...]

    @model_validator(mode="after")
    def unique_host_target_entries(self) -> "BaselineSet":
        pairs = [(entry.host_label, entry.target) for entry in self.entries]
        if len(pairs) != len(set(pairs)):
            raise ValueError("baselines may contain only one entry per host_label and target")
        return self

    def entry_for(self, host_label: str, target: str) -> BaselineEntry:
        for entry in self.entries:
            if entry.host_label == host_label and entry.target == target:
                return entry
        raise BenchmarkSchemaError(
            f"missing benchmark baseline for host_label={host_label!r}, target={target!r}"
        )


REVIEWED_RATCHET_EXCEPTION_APPROVER = "Rand via D3 ratchet exception"


def require_non_decreasing_baselines(previous: BaselineSet, current: BaselineSet) -> None:
    """Reject lowered floors unless an explicitly reviewed exception explains one."""
    if current.revision <= previous.revision:
        raise BenchmarkSchemaError("baseline revision must increase")
    current_entries = {(entry.host_label, entry.target): entry for entry in current.entries}
    for entry in previous.entries:
        replacement = current_entries.get((entry.host_label, entry.target))
        if replacement is None:
            raise BenchmarkSchemaError(
                f"baseline revision may not remove {(entry.host_label, entry.target)!r}"
            )
        if replacement.p50_floor < entry.p50_floor and not (
            replacement.approved_by == REVIEWED_RATCHET_EXCEPTION_APPROVER
            and replacement.rationale is not None
        ):
            raise BenchmarkSchemaError(
                "baseline revision may not lower "
                f"{(entry.host_label, entry.target)!r} without the reviewed D3 exception"
            )


class BaselineRef(BaseModel):
    """Immutable snapshot of the reviewed floor applied to a result."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    revision: int = Field(ge=1)
    p50_floor: float = Field(ge=0)


def require_utc(value: datetime, field: str) -> datetime:
    """Reject naive and non-UTC datetimes at the public artifact boundary."""
    if value.tzinfo is None or value.utcoffset() != timezone.utc.utcoffset(value):
        raise ValueError(f"{field} must be an ISO-8601 UTC timestamp")
    return value


def artifact_id(*, campaign_id: str, target: str) -> str:
    """Derive one safe deterministic per-target public artifact identifier."""
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", campaign_id):
        raise BenchmarkSchemaError("campaign_id must be a safe opaque identifier")
    if target not in {"sqlite", "uds", "tcp", "tcp-tls"}:
        raise BenchmarkSchemaError(f"unknown benchmark target {target!r}")
    return f"{campaign_id}-{target}"


def campaign_id(*, started_at: datetime, host_label: str) -> str:
    """Derive the single UTC-and-host campaign identity shared by runner/report."""
    require_utc(started_at, "started_at")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}", host_label):
        raise BenchmarkSchemaError("host_label must be a safe label")
    return f"{started_at.strftime('%Y%m%dT%H%M%SZ')}-{host_label}"


class BenchmarkRunResult(BaseModel):
    """The v4 public result for exactly one required benchmark target."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[4] = SUMMARY_SCHEMA_VERSION
    campaign_id: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
    host_label: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    os: Literal["macos", "windows", "linux"]
    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    status: Literal["PASS", "FAIL", "INCOMPLETE"]
    incomplete_reason: str | None = None
    generated_at: datetime
    source_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    binary_hashes: dict[str, str]
    frames_per_connection: int = Field(ge=0)
    messages_requested: int = Field(ge=0)
    messages_admitted: int = Field(ge=0)
    messages_durable: int = Field(ge=0)
    metrics: BenchmarkMetrics | None = None
    baseline: BaselineRef
    durability_after_restart: DurabilityAfterRestart | None = None
    direct_sqlite_message_write: DirectSQLiteMessageWrite | None = None

    @field_validator("generated_at")
    @classmethod
    def utc_generated_at(cls, value: datetime) -> datetime:
        return require_utc(value, "generated_at")

    @model_validator(mode="after")
    def consistent_status_and_target_shape(self) -> "BenchmarkRunResult":
        if (self.status == "INCOMPLETE") != (self.incomplete_reason is not None):
            raise ValueError("incomplete_reason is required iff status is INCOMPLETE")
        if self.messages_admitted > self.messages_requested or self.messages_durable > self.messages_admitted:
            raise ValueError("message counts must satisfy durable <= admitted <= requested")
        if self.target == "sqlite" and self.frames_per_connection != 0:
            raise ValueError("sqlite target must have frames_per_connection=0")
        if self.target != "sqlite" and self.frames_per_connection <= 0:
            raise ValueError("network target must have frames_per_connection > 0")
        network_fields = (
            self.metrics.application_wire_bytes if self.metrics else None,
            self.metrics.request_frames_per_second if self.metrics else None,
            self.metrics.connections_per_second if self.metrics else None,
            self.metrics.application_wire_bytes_per_second if self.metrics else None,
        )
        if self.target == "sqlite" and any(value is not None for value in network_fields):
            raise ValueError("sqlite metrics must not invent network values")
        if self.target != "sqlite" and self.metrics is not None and any(
            value is None for value in network_fields
        ):
            raise ValueError("network target metrics require all network values")
        expected = classify_status(
            lifecycle_complete=self.metrics is not None and self.durability_after_restart is not None,
            messages_requested=self.messages_requested,
            messages_admitted=self.messages_admitted,
            messages_durable=self.messages_durable,
            p50_admissions_per_second=(
                None if self.metrics is None else self.metrics.admissions_per_second.p50
            ),
            baseline_p50_floor=self.baseline.p50_floor,
        )
        if self.status != expected:
            raise ValueError(f"status must equal classify_status() output {expected}")
        return self


def required_targets(os_name: str) -> frozenset[str]:
    """Return the unskippable v4 target matrix for an OS identifier."""
    targets = {"sqlite", "tcp", "tcp-tls"}
    if os_name in {"macos", "linux"}:
        targets.add("uds")
    return frozenset(targets)


class BenchmarkCampaign(BaseModel):
    """A complete computer/run benchmark campaign with machine-derived roll-up."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[4] = SUMMARY_SCHEMA_VERSION
    campaign_id: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
    host_label: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    os: Literal["macos", "windows", "linux"]
    phase: str = Field(min_length=1)
    started_at: datetime
    completed_at: datetime | None = None
    source_revision: str = Field(pattern=r"^[0-9a-f]{40}$")
    results: tuple[BenchmarkRunResult, ...]
    status: Literal["PASS", "FAIL", "INCOMPLETE"]

    @field_validator("started_at", "completed_at")
    @classmethod
    def utc_campaign_times(cls, value: datetime | None) -> datetime | None:
        return None if value is None else require_utc(value, "campaign timestamp")

    @model_validator(mode="after")
    def complete_matrix_has_derived_status(self) -> "BenchmarkCampaign":
        targets = {result.target for result in self.results}
        expected_targets = required_targets(self.os)
        if len(targets) != len(self.results):
            raise ValueError("campaign may contain each target only once")
        if any(result.campaign_id != self.campaign_id for result in self.results):
            raise ValueError("every result must belong to its campaign")
        if any(result.host_label != self.host_label or result.os != self.os for result in self.results):
            raise ValueError("every result must match campaign host and OS")
        expected = classify_status(
            required_targets=expected_targets,
            observed_targets=targets,
            target_statuses=tuple(result.status for result in self.results),
        )
        if self.status != expected:
            raise ValueError(f"campaign status must equal derived roll-up {expected}")
        return self


class RatchetPoint(BaseModel):
    """AO2.12 HistoricalRecord ratchet point; see the sprint's normative contract."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    host_label: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    effective_from: datetime
    p50_floor: float = Field(ge=0)
    source_campaign_id: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")

    @field_validator("effective_from")
    @classmethod
    def utc_effective_from(cls, value: datetime) -> datetime:
        return require_utc(value, "effective_from")


class HistoricalResultEntry(BaseModel):
    """AO2.12 HistoricalRecord result wrapper; preserves its v4 result verbatim."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    result: BenchmarkRunResult
    displayed_status: Literal["PASS", "FAIL", "INCOMPLETE"]
    evidence_gap: Literal["durability-counts-missing"] | None
    source_files: tuple[str, ...]

    @field_validator("source_files")
    @classmethod
    def source_files_are_safe_basenames(cls, values: tuple[str, ...]) -> tuple[str, ...]:
        if any(not value or value != Path(value).name for value in values):
            raise ValueError("source_files must contain safe non-empty basenames")
        return values


class HistoricalCampaignEntry(BaseModel):
    """AO2.12 HistoricalRecord campaign entry; see its normative contract."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    campaign: BenchmarkCampaign
    final_best: bool
    results: tuple[HistoricalResultEntry, ...]

    @model_validator(mode="after")
    def results_match_campaign(self) -> "HistoricalCampaignEntry":
        if tuple(entry.result for entry in self.results) != self.campaign.results:
            raise ValueError("historical entries must preserve campaign results exactly")
        return self


class UnattributedEntry(BaseModel):
    """AO2.12 HistoricalRecord entry for a source with no safe campaign mapping."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    source_file: str
    reason: str = Field(min_length=1)

    @field_validator("source_file")
    @classmethod
    def source_file_is_safe_basename(cls, value: str) -> str:
        if not value or value != Path(value).name:
            raise ValueError("source_file must be a safe non-empty basename")
        return value


class HistoricalRecord(BaseModel):
    """AO2.12 normative historical-record contract, consumed unchanged by AO2.11."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[1]
    generated_from_commit: str = Field(pattern=r"^[0-9a-f]{40}$")
    campaigns: tuple[HistoricalCampaignEntry, ...]
    ratchet: tuple[RatchetPoint, ...]
    unattributed: tuple[UnattributedEntry, ...]

    @model_validator(mode="after")
    def chronological_and_monotonic(self) -> "HistoricalRecord":
        starts = [entry.campaign.started_at for entry in self.campaigns]
        if starts != sorted(starts):
            raise ValueError("historical campaigns must be started_at ascending")
        by_host_target: dict[tuple[str, str], list[RatchetPoint]] = {}
        for point in self.ratchet:
            by_host_target.setdefault((point.host_label, point.target), []).append(point)
        for points in by_host_target.values():
            if [point.effective_from for point in points] != sorted(point.effective_from for point in points):
                raise ValueError("ratchet points must be effective_from ascending per host and target")
            if any(after.p50_floor < before.p50_floor for before, after in zip(points, points[1:])):
                raise ValueError("ratchet points must be non-decreasing per host and target")
        return self


class BenchmarkSummary(BaseModel):
    """One immutable public artifact for one transport/frame benchmark run."""

    # This read-only v3 compatibility model intentionally drops retired
    # acceptance metadata from historical artifacts.  v4 remains strict.
    model_config = ConfigDict(extra="ignore", frozen=True)

    schema_version: Literal[LEGACY_SUMMARY_SCHEMA_VERSION] = LEGACY_SUMMARY_SCHEMA_VERSION
    artifact_kind: Literal["send_message_benchmark_summary"] = "send_message_benchmark_summary"
    generated_at: str = Field(min_length=1)
    campaign_id: Optional[str] = Field(
        default=None, pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"
    )
    host_label: str = Field(pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
    transport: Literal["sqlite", "uds", "tcp"]
    peer_wire_security: Optional[Literal["mutual-tls", "plaintext-test"]] = None
    benchmark_target: Optional[Literal["sqlite", "uds", "tcp", "tcp-tls"]] = None
    hook_mode: Optional[Literal["active"]] = None
    frames_per_connection: Literal[1, 2, 4, 8, 16, 64]
    messages_per_connection: int = Field(gt=0)
    requested_messages_per_sample: int = Field(gt=0)
    minimum_sample_count: int = Field(gt=0)
    sample_count: int = Field(ge=0)
    target_duration_s: float = Field(gt=0)
    run_duration_s: float = Field(ge=0)
    source_revision: Optional[str] = Field(default=None, pattern=r"^[0-9a-f]{40}$")
    daemon_version: Optional[str] = None
    host_os: Optional[str] = None
    host_arch: Optional[str] = None
    command: Optional[str] = None
    execution_daemon: Optional[Literal["shipped_atm_daemon", "direct_production_writer"]] = None
    worker_limit: Optional[int] = Field(default=None, gt=0)
    host_state_isolation: Optional[str] = None
    doctor_status: Optional[Literal["passed"]] = None
    doctor_after_restart_status: Optional[Literal["passed"]] = None
    durability_after_restart: Optional[DurabilityAfterRestart] = None
    direct_sqlite_message_write: Optional[DirectSQLiteMessageWrite] = None
    metrics: Optional[BenchmarkMetrics] = None
    passed: bool
    failure: Optional[str] = None
    cleanup_failure: Optional[str] = None

    @field_validator("generated_at")
    @classmethod
    def utc_timestamp(cls, value: str) -> str:
        if not value.endswith("Z") or "T" not in value:
            raise ValueError("generated_at must be an ISO-8601 UTC timestamp")
        return value

    @model_validator(mode="after")
    def run_matches_metrics(self) -> "BenchmarkSummary":
        """Validate v3 shape without reenacting its retired acceptance policy."""
        if self.benchmark_target == "sqlite" and self.transport != "sqlite":
            raise ValueError("sqlite target must use the direct production-writer profile")
        if self.benchmark_target == "uds" and self.transport != "uds":
            raise ValueError("uds target must use the public UDS profile")
        if self.benchmark_target == "tcp" and (
            self.transport != "tcp" or self.peer_wire_security != "plaintext-test"
        ):
            raise ValueError("tcp target must use the tcp plaintext-test profile")
        if self.benchmark_target == "tcp-tls" and (
            self.transport != "tcp" or self.peer_wire_security != "mutual-tls"
        ):
            raise ValueError("tcp-tls target must use the tcp mutual-TLS profile")
        if self.metrics is None:
            if self.passed or self.failure is None:
                raise ValueError("a result without samples must be a failed run with a failure")
            return self
        if self.sample_count != self.metrics.interval_count:
            raise ValueError("sample_count must equal metrics.interval_count")
        if self.durability_after_restart is not None and self.durability_after_restart.expected_accepted_count != self.metrics.accepted_count:
            raise ValueError("durability expected count must equal accepted_count")
        return self


def percentile(values: list[float], fraction: float) -> float:
    """Use the runner's existing nearest-rank percentile convention."""
    ordered = sorted(values)
    if not ordered:
        return 0.0
    return ordered[min(len(ordered) - 1, int(len(ordered) * fraction))]


def distribution(values: list[float]) -> dict[str, float]:
    """Summarize an interval metric without retaining the interval trace."""
    ordered = sorted(values)
    if not ordered:
        raise BenchmarkSchemaError("cannot summarize an empty interval trace")
    middle = len(ordered) // 2
    p50 = ordered[middle] if len(ordered) % 2 else (ordered[middle - 1] + ordered[middle]) / 2
    return {
        "min": ordered[0],
        "p50": p50,
        "p95": percentile(ordered, 0.95),
        "p99": percentile(ordered, 0.99),
        "max": ordered[-1],
    }


def direct_sqlite_write_from_evidence(
    evidence: dict[str, Any],
) -> DirectSQLiteMessageWrite | None:
    """Extract the optional direct-writer measurement from raw runner facts.

    This helper deliberately has no schema-version semantics.  It is shared by
    the v4 writer and the read-only legacy compactor so the ordinary runner
    never has to emit a v3 file just to reuse its factual aggregation logic.
    """
    decomposition = evidence.get("decomposition", {})
    value = evidence.get("direct_sqlite_message_write")
    if value is None and isinstance(decomposition, dict):
        value = decomposition.get("async_storage_admission")
    if value is None:
        return None
    try:
        return DirectSQLiteMessageWrite.model_validate(value)
    except ValidationError as error:
        raise BenchmarkSchemaError(str(error)) from error


def metrics_from_evidence(evidence: dict[str, Any]) -> BenchmarkMetrics | None:
    """Aggregate verbose runner intervals into public v4 metric facts.

    Returning ``None`` represents an incomplete lifecycle.  Callers own the
    status decision; this structural helper neither applies a baseline nor
    writes a versioned artifact.
    """
    try:
        profile = evidence["runs"][0]
        intervals = profile["intervals"]
    except (IndexError, KeyError, TypeError):
        intervals = []
    if not isinstance(intervals, list):
        raise BenchmarkSchemaError("evidence intervals must be a list")
    if not intervals:
        return None

    def metric(name: str, default: float = 0.0) -> list[float]:
        return [float(interval.get(name, default)) for interval in intervals]

    def latency(name: str, fallback: str | None = None) -> list[float]:
        return [
            float(interval.get("latency_ms", {}).get(name, interval.get("latency_ms", {}).get(fallback, 0.0)))
            for interval in intervals
        ]

    request_bytes = sum(
        int(interval.get("application_wire_bytes", {}).get("request", 0))
        for interval in intervals
    )
    response_bytes = sum(
        int(interval.get("application_wire_bytes", {}).get("response", 0))
        for interval in intervals
    )
    first_failure = next(
        (
            public_string(str(interval["first_failure"]))
            for interval in intervals
            if interval.get("first_failure")
        ),
        None,
    )
    try:
        return BenchmarkMetrics.model_validate({
            "interval_count": len(intervals),
            "passed_interval_count": sum(bool(interval.get("passed")) for interval in intervals),
            "accepted_count": sum(int(interval.get("accepted_count", 0)) for interval in intervals),
            "requested_count": sum(int(interval.get("requested_count", 0)) for interval in intervals),
            "response_count": sum(int(interval.get("response_count", 0)) for interval in intervals),
            "connection_count": sum(int(interval.get("connections", 0)) for interval in intervals),
            "application_wire_bytes": {
                "request": request_bytes,
                "response": response_bytes,
                "total": request_bytes + response_bytes,
            },
            "admissions_per_second": distribution(metric("admissions_per_second")),
            "request_frames_per_second": distribution(
                metric("request_frames_per_second", metric("admissions_per_second")[0])
            ),
            "connections_per_second": distribution(metric("connections_per_second")),
            "application_wire_bytes_per_second": distribution(
                metric("application_wire_bytes_per_second", metric("bytes_per_second")[0])
            ),
            "time_to_send_1k_s": distribution(
                metric("time_to_send_1k_s", metric("elapsed_seconds")[0])
            ),
            "interval_latency_ms": {
                "min": min(latency("min")),
                "p50": distribution(latency("p50"))["p50"],
                "p95": distribution(latency("p95"))["p95"],
                "p99": distribution(latency("p99", "p95"))["p99"],
                "max": max(latency("max")),
            },
            "first_failure": first_failure,
        })
    except ValidationError as error:
        raise BenchmarkSchemaError(str(error)) from error


def compact_evidence(evidence: dict[str, Any]) -> BenchmarkSummary:
    """Convert one v2 verbose runner record into its v3 public summary."""
    metrics = metrics_from_evidence(evidence)
    if metrics is None:
        return failed_summary(evidence)
    summary = {
        "generated_at": evidence["generated_at"],
        "campaign_id": evidence.get("campaign_id"),
        "host_label": evidence["host_label"],
        "transport": evidence["transport"],
        "peer_wire_security": evidence.get("peer_wire_security"),
        "benchmark_target": evidence.get("benchmark_target"),
        "hook_mode": evidence.get("hook_mode"),
        "frames_per_connection": evidence["frames_per_connection"],
        "messages_per_connection": evidence.get("messages_per_connection", evidence["frames_per_connection"]),
        "requested_messages_per_sample": (
            evidence.get("requested_messages_per_sample")
            or metrics.requested_count
            or 1_000
        ),
        "minimum_sample_count": evidence.get("minimum_sample_count") or metrics.interval_count,
        "sample_count": metrics.interval_count,
        "target_duration_s": evidence.get("target_duration_s", evidence["run_duration_s"]),
        "run_duration_s": evidence["run_duration_s"],
        "source_revision": evidence.get("source_revision"),
        "daemon_version": evidence.get("daemon_version"),
        "host_os": evidence.get("host_os"),
        "host_arch": evidence.get("host_arch"),
        "command": evidence.get("command"),
        "execution_daemon": evidence.get("execution_daemon"),
        "worker_limit": evidence.get("worker_limit"),
        "host_state_isolation": evidence.get("host_state_isolation"),
        "doctor_status": evidence.get("doctor_status"),
        "doctor_after_restart_status": evidence.get("doctor_after_restart", {}).get("status"),
        "durability_after_restart": evidence.get("durability_after_restart"),
        "direct_sqlite_message_write": direct_sqlite_write_from_evidence(evidence),
        "metrics": metrics,
        "passed": bool(evidence.get("passed", False)),
        "failure": public_string(str(evidence["failure"])) if evidence.get("failure") else None,
        "cleanup_failure": public_string(str(evidence["cleanup_failure"])) if evidence.get("cleanup_failure") else None,
    }
    try:
        return BenchmarkSummary.model_validate(summary)
    except ValidationError as error:
        raise BenchmarkSchemaError(str(error)) from error


def failed_summary(evidence: dict[str, Any]) -> BenchmarkSummary:
    """Represent a failed setup run without inventing performance metrics."""
    try:
        summary = {
            "generated_at": evidence["generated_at"],
            "campaign_id": evidence.get("campaign_id"),
            "host_label": evidence["host_label"],
            "transport": evidence["transport"],
            "peer_wire_security": evidence.get("peer_wire_security"),
            "benchmark_target": evidence.get("benchmark_target"),
            "hook_mode": evidence.get("hook_mode"),
            "frames_per_connection": evidence["frames_per_connection"],
            "messages_per_connection": evidence.get("messages_per_connection", evidence["frames_per_connection"]),
            "requested_messages_per_sample": evidence.get("requested_messages_per_sample", 1_000),
            "minimum_sample_count": evidence.get("minimum_sample_count", 1),
            "sample_count": 0,
            "target_duration_s": evidence.get("target_duration_s", 20.0),
            "run_duration_s": 0.0,
            "source_revision": evidence.get("source_revision"),
            "daemon_version": evidence.get("daemon_version"),
            "host_os": evidence.get("host_os"),
            "host_arch": evidence.get("host_arch"),
            "command": evidence.get("command"),
            "execution_daemon": evidence.get("execution_daemon"),
            "worker_limit": evidence.get("worker_limit"),
            "host_state_isolation": evidence.get("host_state_isolation"),
            "doctor_status": evidence.get("doctor_status"),
            "doctor_after_restart_status": evidence.get("doctor_after_restart", {}).get("status"),
            "direct_sqlite_message_write": evidence.get("direct_sqlite_message_write")
            if evidence.get("direct_sqlite_message_write") is not None
            else (
                evidence.get("decomposition", {}).get("async_storage_admission")
                if isinstance(evidence.get("decomposition"), dict)
                else None
            ),
            "passed": False,
            "failure": public_string(str(evidence.get("failure") or "benchmark did not reach an interval")),
            "cleanup_failure": public_string(str(evidence["cleanup_failure"])) if evidence.get("cleanup_failure") else None,
        }
        return BenchmarkSummary.model_validate(summary)
    except (KeyError, ValidationError) as error:
        raise BenchmarkSchemaError(str(error)) from error


def summary_json(evidence: dict[str, Any]) -> str:
    """Return canonical compact JSON after Pydantic validation."""
    return compact_evidence(evidence).model_dump_json(indent=2) + "\n"

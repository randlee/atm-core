"""Versioned, public benchmark-summary schema shared by runner and migration."""
from __future__ import annotations

from math import isclose
from typing import Any, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field, ValidationError, field_validator, model_validator

from scripts.public_redaction import public_string


SUMMARY_SCHEMA_VERSION = 3
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
    connection_count: int = Field(ge=0)
    application_wire_bytes: WireBytes
    admissions_per_second: MetricDistribution
    request_frames_per_second: MetricDistribution
    connections_per_second: MetricDistribution
    application_wire_bytes_per_second: MetricDistribution
    time_to_send_1k_s: MetricDistribution
    interval_latency_ms: MetricDistribution
    first_failure: Optional[str] = None

    @model_validator(mode="after")
    def passed_count_is_bounded(self) -> "BenchmarkMetrics":
        if self.passed_interval_count > self.interval_count:
            raise ValueError("passed_interval_count cannot exceed interval_count")
        return self


class BenchmarkThresholds(BaseModel):
    """Acceptance gates evaluated from the same interval trace."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    admissions_per_second_minimum: float = Field(ge=0)
    median_admissions_per_second: float = Field(ge=0)
    baseline_median_admissions_per_second: Optional[float] = Field(default=None, ge=0)
    baseline_passed: bool
    admission_passed: bool
    comparison_median_admissions_per_second: Optional[float] = Field(default=None, ge=0)
    comparison_ratio: Optional[float] = Field(default=None, ge=0)
    comparison_target_admissions_per_second: Optional[float] = Field(default=None, ge=0)
    comparison_strict: Optional[bool] = None
    comparison_required: Optional[bool] = None
    comparison_passed: bool
    passed: bool


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


class BenchmarkSummary(BaseModel):
    """One immutable public artifact for one transport/frame benchmark run."""

    model_config = ConfigDict(extra="forbid", frozen=True)

    schema_version: Literal[SUMMARY_SCHEMA_VERSION] = SUMMARY_SCHEMA_VERSION
    artifact_kind: Literal["send_message_benchmark_summary"] = "send_message_benchmark_summary"
    generated_at: str = Field(min_length=1)
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
    comparison_source_revision: Optional[str] = Field(default=None, pattern=r"^[0-9a-f]{40}$")
    comparison_host_label: Optional[str] = Field(
        default=None, pattern=r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$"
    )
    worker_limit: Optional[int] = Field(default=None, gt=0)
    host_state_isolation: Optional[str] = None
    doctor_status: Optional[Literal["passed"]] = None
    doctor_after_restart_status: Optional[Literal["passed"]] = None
    durability_after_restart: Optional[DurabilityAfterRestart] = None
    direct_sqlite_message_write: Optional[DirectSQLiteMessageWrite] = None
    thresholds: Optional[BenchmarkThresholds] = None
    metrics: Optional[BenchmarkMetrics] = None
    passed: bool
    benchmark_evidence_failure_code: Optional[
        Literal["missing_compatible_plaintext_baseline"]
    ] = None
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
        expected_passed = (
            self.metrics.passed_interval_count == self.sample_count
            and self.failure is None
            and self.cleanup_failure is None
            and (self.thresholds is None or self.thresholds.passed)
        )
        if self.passed != expected_passed:
            raise ValueError("passed must agree with metrics and failure")
        if self.durability_after_restart is not None and self.durability_after_restart.expected_accepted_count != self.metrics.accepted_count:
            raise ValueError("durability expected count must equal accepted_count")
        if self.thresholds is not None:
            if not isclose(self.thresholds.median_admissions_per_second, self.metrics.admissions_per_second.p50):
                raise ValueError("threshold median must equal admissions_per_second.p50")
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


def compact_evidence(evidence: dict[str, Any]) -> BenchmarkSummary:
    """Convert one v2 verbose runner record into its v3 public summary."""
    try:
        profile = evidence["runs"][0]
        intervals = profile["intervals"]
    except (IndexError, KeyError, TypeError):
        profile, intervals = {}, []
    if not isinstance(intervals, list):
        raise BenchmarkSchemaError("evidence intervals must be a list")
    if not intervals:
        return failed_summary(evidence)

    def metric(name: str, default: float = 0.0) -> list[float]:
        return [float(interval.get(name, default)) for interval in intervals]

    def latency(name: str, fallback: str | None = None) -> list[float]:
        return [
            float(interval.get("latency_ms", {}).get(name, interval.get("latency_ms", {}).get(fallback, 0.0)))
            for interval in intervals
        ]

    request_bytes = sum(int(interval.get("application_wire_bytes", {}).get("request", 0)) for interval in intervals)
    response_bytes = sum(int(interval.get("application_wire_bytes", {}).get("response", 0)) for interval in intervals)
    first_failure = next((public_string(str(interval["first_failure"])) for interval in intervals if interval.get("first_failure")), None)
    thresholds = evidence.get("thresholds")
    decomposition = evidence.get("decomposition", {})
    # Runner evidence carries this diagnostic below `decomposition`.  Accepting
    # the already-compact spelling too makes a report rebuild lossless when it
    # reprocesses a published artifact.
    direct_sqlite_message_write = evidence.get("direct_sqlite_message_write")
    if direct_sqlite_message_write is None and isinstance(decomposition, dict):
        direct_sqlite_message_write = decomposition.get("async_storage_admission")
    summary = {
        "generated_at": evidence["generated_at"],
        "host_label": evidence["host_label"],
        "transport": evidence["transport"],
        "peer_wire_security": evidence.get("peer_wire_security"),
        "benchmark_target": evidence.get("benchmark_target"),
        "hook_mode": evidence.get("hook_mode"),
        "frames_per_connection": evidence["frames_per_connection"],
        "messages_per_connection": evidence.get("messages_per_connection", evidence["frames_per_connection"]),
        "requested_messages_per_sample": evidence.get("requested_messages_per_sample", intervals[0].get("requested_count", 1_000)),
        "minimum_sample_count": evidence.get("minimum_sample_count", len(intervals)),
        "sample_count": len(intervals),
        "target_duration_s": evidence.get("target_duration_s", evidence["run_duration_s"]),
        "run_duration_s": evidence["run_duration_s"],
        "source_revision": evidence.get("source_revision"),
        "daemon_version": evidence.get("daemon_version"),
        "host_os": evidence.get("host_os"),
        "host_arch": evidence.get("host_arch"),
        "command": evidence.get("command"),
        "execution_daemon": evidence.get("execution_daemon"),
        "comparison_source_revision": evidence.get("comparison_source_revision"),
        "comparison_host_label": evidence.get("comparison_host_label"),
        "worker_limit": evidence.get("worker_limit"),
        "host_state_isolation": evidence.get("host_state_isolation"),
        "doctor_status": evidence.get("doctor_status"),
        "doctor_after_restart_status": evidence.get("doctor_after_restart", {}).get("status"),
        "durability_after_restart": evidence.get("durability_after_restart"),
        "direct_sqlite_message_write": direct_sqlite_message_write,
        "thresholds": thresholds,
        "metrics": {
            "interval_count": len(intervals),
            "passed_interval_count": sum(bool(interval.get("passed")) for interval in intervals),
            "accepted_count": sum(int(interval.get("accepted_count", 0)) for interval in intervals),
            "requested_count": sum(int(interval.get("requested_count", 0)) for interval in intervals),
            "response_count": sum(int(interval.get("response_count", 0)) for interval in intervals),
            "connection_count": sum(int(interval.get("connections", 0)) for interval in intervals),
            "application_wire_bytes": {"request": request_bytes, "response": response_bytes, "total": request_bytes + response_bytes},
            "admissions_per_second": distribution(metric("admissions_per_second")),
            "request_frames_per_second": distribution(metric("request_frames_per_second", metric("admissions_per_second")[0] if intervals else 0.0)),
            "connections_per_second": distribution(metric("connections_per_second")),
            "application_wire_bytes_per_second": distribution(metric("application_wire_bytes_per_second", metric("bytes_per_second")[0] if intervals else 0.0)),
            "time_to_send_1k_s": distribution(metric("time_to_send_1k_s", metric("elapsed_seconds")[0] if intervals else 0.0)),
            "interval_latency_ms": {
                "min": min(latency("min")),
                "p50": distribution(latency("p50"))["p50"],
                "p95": distribution(latency("p95"))["p95"],
                "p99": distribution(latency("p99", "p95"))["p99"],
                "max": max(latency("max")),
            },
            "first_failure": first_failure,
        },
        "passed": bool(evidence.get("passed", False)),
        "benchmark_evidence_failure_code": evidence.get("benchmark_evidence_failure_code"),
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
            "comparison_source_revision": evidence.get("comparison_source_revision"),
            "comparison_host_label": evidence.get("comparison_host_label"),
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
            "benchmark_evidence_failure_code": evidence.get("benchmark_evidence_failure_code"),
            "failure": public_string(str(evidence.get("failure") or "benchmark did not reach an interval")),
            "cleanup_failure": public_string(str(evidence["cleanup_failure"])) if evidence.get("cleanup_failure") else None,
        }
        return BenchmarkSummary.model_validate(summary)
    except (KeyError, ValidationError) as error:
        raise BenchmarkSchemaError(str(error)) from error


def summary_json(evidence: dict[str, Any]) -> str:
    """Return canonical compact JSON after Pydantic validation."""
    return compact_evidence(evidence).model_dump_json(indent=2) + "\n"

#!/usr/bin/env python3
"""Prove the public local ATM admission boundary accepts 1,000 writes/second.

This runner is deliberately a *clean-user* smoke gate. ADR-026 makes the
daemon and its SQLite store OS-user-owned, not ``ATM_HOME``-owned. It therefore
refuses to run beside an ambient daemon; changing ``ATM_HOME`` alone would not
isolate a developer's real mail database.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import closing
from dataclasses import dataclass, replace
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
from queue import Empty, Queue
import re
import shutil
import socket
import sqlite3
import ssl
import subprocess
import sys
import tempfile
from threading import Lock, Thread
import time
from typing import Any, Callable
import uuid

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_EVIDENCE_DIR = ROOT / "site" / "reports" / "send-message-benchmark"
DEFAULT_RAW_EVIDENCE_DIR = ROOT / "artifacts" / "benchmark" / "send-message-benchmark"
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_schema import (
    BaselineEntry,
    BaselineSet,
    BaselineRef,
    BenchmarkCampaign,
    BenchmarkRunResult,
    BenchmarkSchemaError,
    DurabilityAfterRestart,
    artifact_id,
    campaign_id as derive_campaign_id,
    compact_evidence,
    direct_sqlite_write_from_evidence,
    distribution,
    metrics_from_evidence,
    required_targets,
)
from scripts.smoke.benchmark_policy import classify_status, profile_median_admissions_per_second
from scripts.smoke.benchmark_account import (
    BenchmarkAccount,
    BenchmarkAccountError,
    bootstrap_benchmark_account,
    require_benchmark_account,
)
from scripts.smoke.benchmark_baselines import load_baselines
from scripts.report_runtime import source_revision as _git_source_revision
from scripts.smoke.benchmark_mtls import BenchmarkMtlsError, regenerate_mtls_identity
from scripts.smoke.benchmark_snapshot import (
    BenchmarkSnapshotError,
    VerifiedSnapshot,
    checkpoint_closed_database,
    create_verified_snapshot,
    restore_verified_snapshot,
    verify_active_snapshot,
)

if os.name != "nt":
    import pwd

try:
    import resource
except ImportError:  # Windows has no POSIX rlimit API.
    resource = None

from scripts.smoke.daemon_lifecycle import (
    assert_no_process_leak,
    count_atm_daemon_processes,
    require_clean_host_daemon_state,
    terminate_process,
)
from scripts.smoke.smoke_common import SmokeError, command_result

_PUBLIC_NAMESPACE: dict[str, Any] | None = None


def _public(name: str, fallback: Any) -> Any:
    return _PUBLIC_NAMESPACE.get(name, fallback) if _PUBLIC_NAMESPACE is not None else fallback


INTERVALS = 10
ADMISSIONS_PER_INTERVAL = 1_000
TARGET_PROFILE_DURATION_SECONDS = 20.0
# An f8 sample issues ceil(1_000 / 8) = 125 independent connections.  The
# ordinary benchmark needs enough client workers to occupy all of them; 64
# underdrives every lane and makes the result incomparable with the reviewed
# f8/512 physical baselines.
DEFAULT_WORKERS = 512
# Keep the benchmark's HTTP/1.1 pipeline below the local socket's bidirectional
# buffer capacity. The sender writes one bounded batch, then the reader drains
# every matching response before the next batch.
MAX_IN_FLIGHT_REQUESTS = 8
READY_TIMEOUT_SECONDS = 30.0
CAPACITY_ROOT_PREFIX = "atm-capacity-"
SPARSE_FRAMES_PER_CONNECTION = (1, 2, 4, 8, 16, 64)
SUSTAINED_MESSAGE_COUNTS = (10_000, 100_000)
DAEMON_OUTPUT_TAIL_LINES = 200
PEER_WIRE_SECURITY_MODES = ("mutual-tls", "plaintext-test")
DIRECT_PEER_TCP_PORT = 43_101
CAPACITY_DIRECT_PEER_PORT_ENV = "ATM_CAPACITY_DIRECT_PEER_PORT"
CROCKFORD_BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
BENCHMARK_TARGETS = {
    # SQLite is a direct-storage target, so it does not select a peer-wire
    # mode. The daemon itself still starts in its normal secure default.
    "sqlite": ("sqlite", None),
    "uds": ("uds", "mutual-tls"),
    "tcp": ("tcp", "plaintext-test"),
    "tcp-tls": ("tcp", "mutual-tls"),
}
BASELINES_PATH = DEFAULT_EVIDENCE_DIR / "baselines.json"
DAEMON_SWITCH = ROOT / ".claude" / "skills" / "daemon-switch" / "scripts" / "daemon-switch.py"
# The daemon-switch control plane can legitimately wait through its documented
# launchctl unload/owner-repair windows (up to 20s + two 20x2s polls).  Its
# outer timeout must cover that bounded recovery path; otherwise the runner
# reports a false benchmark failure while the switch is still repairing the
# selected singleton.
MANAGED_DAEMON_TIMEOUT_SECONDS = 120.0
DIAGNOSTIC_SAMPLE_COUNT = 3
DIAGNOSTIC_DURATION_SECONDS = 3.0
# A connection worker owns one client socket while its corresponding daemon
# child owns the peer socket. Leave enough descriptors for the Python runner,
# subprocess pipes, and the bounded daemon control plane instead of scheduling
# more simultaneous connections than the OS account can open.
DESCRIPTOR_RESERVE = 64


@dataclass(frozen=True)
class AdmissionResult:
    status: int
    elapsed_ms: float
    failure: str | None = None
    request_bytes: int = 0
    response_bytes: int = 0
    response_summary: str | None = None


@dataclass(frozen=True)
class HttpRequest:
    """One public HTTP request with its documented success response."""

    path: str
    body: bytes
    expected_status: int


@dataclass(frozen=True)
class LocalEndpoint:
    """One public daemon endpoint, with peer-wire facts when applicable."""

    kind: str
    address: str | tuple[str, int]
    capability: str | None = None
    direct_peer: bool = False
    tls_server_name: str | None = None
    tls_certificate_bundle: Path | None = None


@dataclass(frozen=True)
class DisposableMtlsIdentity:
    """One benchmark-only identity installed before an mTLS daemon starts.

    The PEM bundle is created below the runner's disposable state and is never
    copied into evidence.  It is used both by the daemon's configured local
    identity and by the physical benchmark client, which therefore proves the
    actual mTLS listener rather than a plaintext lookalike.
    """

    host: str
    certificate_bundle: Path
    fingerprint: str


@dataclass(frozen=True)
class CapacityRoster:
    """Unique roster names for one isolated benchmark profile."""

    run_id: str
    team: str
    agent: str
    recipient: str

    @classmethod
    def unique(cls) -> "CapacityRoster":
        suffix = uuid.uuid4().hex[:12]
        return cls(
            run_id=suffix,
            team=f"capacity-team-{suffix}",
            agent=f"capacity-agent-{suffix}",
            recipient=f"capacity-recipient-{suffix}",
        )


DEFAULT_CAPACITY_ROSTER = CapacityRoster(
    run_id="default",
    team="capacity-team",
    agent="capacity-agent",
    recipient="capacity-recipient",
)


@dataclass(frozen=True)
class CapacityRunResult:
    """One target's exit status plus both immutable evidence references.

    Iteration preserves the historical two-value ``code, compact_path`` call
    sites while a complete-suite ledger can retain the raw trace hash too.
    """

    code: int
    compact_evidence_path: Path
    raw_evidence_path: Path
    result: BenchmarkRunResult | None = None

    def __iter__(self):
        yield self.code
        yield self.compact_evidence_path


@dataclass(frozen=True)
class V4EmissionContext:
    """All caller-owned facts required for one direct v4 target publication."""

    target: str
    campaign_id: str
    os_name: str
    baseline: BaselineEntry
    binary_hashes: dict[str, str]


def benchmark_os() -> str:
    """Normalize the runner OS name to the public v4 contract vocabulary."""
    if os.name == "nt":
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    return "linux"


def binary_hashes() -> dict[str, str]:
    """Record the exact release executables measured by a v4 campaign."""
    try:
        return {
            "atm": hashlib.sha256(release_binary("atm").read_bytes()).hexdigest(),
            "atm-daemon": hashlib.sha256(release_binary("atm-daemon").read_bytes()).hexdigest(),
        }
    except OSError as error:
        raise SmokeError(f"could not hash benchmark release binaries: {error}") from error


def atomic_json(path: Path, payload: dict[str, Any]) -> None:
    """Publish a validated immutable JSON artifact or fail before partial output."""
    content = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(path)


def v4_result_from_evidence(
    evidence: dict[str, Any],
    *,
    context: V4EmissionContext,
) -> BenchmarkRunResult:
    """Build the public v4 result directly from the runner's factual trace.

    The normal matrix never persists a legacy summary and then migrates it.
    ``metrics_from_evidence`` is merely in-memory aggregation of the verbose
    trace, shared with the read-only v1-v3 compatibility loader.
    """
    try:
        metrics = metrics_from_evidence(evidence)
        durability_raw = evidence.get("durability_after_restart")
        durability = (
            None
            if durability_raw is None
            else DurabilityAfterRestart.model_validate(durability_raw)
        )
        direct_sqlite = direct_sqlite_write_from_evidence(evidence)
    except BenchmarkSchemaError as error:
        raise SmokeError(f"could not summarize benchmark evidence: {error}") from error
    if metrics is not None and context.target == "sqlite":
        metric_values = metrics.model_dump()
        for field in (
            "connection_count", "application_wire_bytes", "request_frames_per_second",
            "connections_per_second", "application_wire_bytes_per_second",
        ):
            metric_values[field] = None
        metrics = type(metrics).model_validate(metric_values)
    requested = 0 if metrics is None else metrics.requested_count
    admitted = 0 if metrics is None else metrics.accepted_count
    durable = 0 if durability is None else durability.observed_mailbox_count
    complete = metrics is not None and durability is not None
    try:
        generated_at = datetime.fromisoformat(str(evidence["generated_at"]).replace("Z", "+00:00"))
    except (KeyError, ValueError) as error:
        raise SmokeError(f"benchmark evidence has invalid generated_at: {error}") from error
    status = classify_status(
        lifecycle_complete=complete,
        messages_requested=requested,
        messages_admitted=admitted,
        messages_durable=durable,
        p50_admissions_per_second=(None if metrics is None else metrics.admissions_per_second.p50),
        baseline_p50_floor=context.baseline.p50_floor,
    )
    if evidence.get("failure") and status == "FAIL":
        status = "INCOMPLETE"
    return BenchmarkRunResult(
        campaign_id=context.campaign_id,
        host_label=str(evidence["host_label"]),
        os=context.os_name,
        target=context.target,
        status=status,
        incomplete_reason=(
            str(evidence.get("failure"))
            if evidence.get("failure")
            else (None if complete else "benchmark did not complete every lifecycle stage")
        ),
        generated_at=generated_at,
        source_revision=str(evidence.get("source_revision") or source_revision()),
        binary_hashes=context.binary_hashes,
        frames_per_connection=(
            0 if context.target == "sqlite" else int(evidence["frames_per_connection"])
        ),
        messages_requested=requested,
        messages_admitted=admitted,
        messages_durable=durable,
        metrics=metrics,
        baseline=BaselineRef(revision=1, p50_floor=context.baseline.p50_floor),
        durability_after_restart=durability,
        direct_sqlite_message_write=direct_sqlite,
    )


def write_v4_evidence(
    directory: Path, evidence: dict[str, Any], context: V4EmissionContext,
) -> tuple[Path, BenchmarkRunResult]:
    """Validate and atomically publish the ordinary matrix's direct v4 output."""
    try:
        result = v4_result_from_evidence(evidence, context=context)
    except (KeyError, ValueError) as error:
        # Preserve malformed-lane evidence and let the suite continue.  Keep
        # the offending counts in the reason while projecting a schema-valid
        # INCOMPLETE result for publication.
        evidence["passed"] = False
        evidence["failure"] = f"count invariant violation (real counts are recorded): {error}"
        result = v4_result_from_evidence(evidence, context=context)
    if result.status == "FAIL" and result.messages_durable > result.messages_admitted:
        result = result.model_copy(update={
            "status": "INCOMPLETE",
            "incomplete_reason": (
                f"count invariant violation: requested={result.messages_requested}, "
                f"admitted={result.messages_admitted}, durable={result.messages_durable}"
            ),
        })
    destination = directory / f"{artifact_id(campaign_id=context.campaign_id, target=context.target)}.json"
    atomic_json(destination, result.model_dump(mode="json"))
    return destination, result


@dataclass
class HostStateBackup:
    """Retired unsafe host-state swapping interface.

    A transient directory move is not a durable backup. Benchmarks must not
    make room for their disposable state by changing the primary database.
    """

    state_root: Path
    backup_root: Path | None

    @classmethod
    def begin(cls) -> "HostStateBackup":
        raise SmokeError(
            "refusing to replace the current OS user's primary ATM database; "
            "run the benchmark under a dedicated clean OS user with "
            "ATM_CAPACITY_ISOLATED_OS_USER=1"
        )

    def restore(self) -> None:
        raise SmokeError("host-state benchmark backup/restore is retired as unsafe")


@dataclass(frozen=True)
class ManagedDaemonOptions:
    """The existing singleton daemon selected by the daemon-switch skill."""

    service: str
    launch_agent_plist: Path | None = None
    cli_link: Path | None = None
    daemon_link: Path | None = None
    repair_orphan: bool = False

    def command_arguments(self) -> list[str]:
        if not self.service.strip():
            raise SmokeError("backup/restore capacity mode requires a managed daemon --service")
        arguments = ["--service", self.service]
        if self.launch_agent_plist is not None:
            arguments.extend(["--launch-agent-plist", str(self.launch_agent_plist)])
        if self.cli_link is not None:
            arguments.extend(["--cli-link", str(self.cli_link)])
        if self.daemon_link is not None:
            arguments.extend(["--daemon-link", str(self.daemon_link)])
        if self.repair_orphan:
            arguments.append("--repair-orphan")
        return arguments


@dataclass
class LaunchAgentPeerWireOverride:
    """One disposable macOS LaunchAgent copy for a benchmark wire mode.

    The managed daemon remains the selected CLI/daemon pair.  Only this copied
    plist gains the normal daemon launch argument for the selected benchmark
    mode.  The operator-owned source plist is never written; recovery verifies
    that its original bytes still exist before it is launched again.
    """

    source_path: Path
    source_bytes: bytes
    temporary_directory: tempfile.TemporaryDirectory[str]
    override_path: Path

    @classmethod
    def create(
        cls,
        source_path: Path,
        peer_wire_security: str,
        managed_log_level: str | None = None,
    ) -> "LaunchAgentPeerWireOverride":
        source = source_path.expanduser()
        try:
            source_bytes = source.read_bytes()
            payload = plistlib.loads(source_bytes)
        except (OSError, plistlib.InvalidFileException) as error:
            raise SmokeError(f"could not read managed LaunchAgent plist {source}: {error}") from error
        if not isinstance(payload, dict):
            raise SmokeError(f"managed LaunchAgent plist {source} must contain a dictionary")
        arguments = payload.get("ProgramArguments")
        if not isinstance(arguments, list) or not all(isinstance(value, str) for value in arguments):
            raise SmokeError(f"managed LaunchAgent plist {source} must contain string ProgramArguments")
        if not any(value.endswith("atm-daemon") for value in arguments):
            raise SmokeError(f"managed LaunchAgent plist {source} does not launch atm-daemon")

        adjusted_arguments: list[str] = []
        position = 0
        while position < len(arguments):
            argument = arguments[position]
            if argument != "--peer-wire-security":
                adjusted_arguments.append(argument)
                position += 1
                continue
            if position + 1 >= len(arguments):
                raise SmokeError(
                    f"managed LaunchAgent plist {source} has --peer-wire-security without a value"
                )
            position += 2
        adjusted_arguments.extend(("--peer-wire-security", peer_wire_security))
        payload["ProgramArguments"] = adjusted_arguments
        if managed_log_level is not None:
            environment = payload.get("EnvironmentVariables", {})
            if not isinstance(environment, dict) or not all(
                isinstance(key, str) and isinstance(value, str)
                for key, value in environment.items()
            ):
                raise SmokeError(
                    f"managed LaunchAgent plist {source} must contain string EnvironmentVariables"
                )
            payload["EnvironmentVariables"] = {**environment, "ATM_LOG": managed_log_level}

        temporary_directory = tempfile.TemporaryDirectory(prefix="atm-capacity-launch-")
        override = Path(temporary_directory.name) / source.name
        try:
            with override.open("wb") as handle:
                plistlib.dump(payload, handle, fmt=plistlib.FMT_XML, sort_keys=False)
        except OSError as error:
            temporary_directory.cleanup()
            raise SmokeError(f"could not write benchmark LaunchAgent override {override}: {error}") from error
        return cls(source, source_bytes, temporary_directory, override)

    def assert_source_unchanged(self) -> None:
        try:
            current_bytes = self.source_path.read_bytes()
        except OSError as error:
            raise SmokeError(
                f"could not re-read original managed LaunchAgent plist {self.source_path}: {error}"
            ) from error
        if current_bytes != self.source_bytes:
            raise SmokeError(
                "original managed LaunchAgent plist changed during benchmark; refusing to claim exact restoration"
            )

    def cleanup(self) -> None:
        self.temporary_directory.cleanup()


def daemon_switch_result(
    action: str,
    options: ManagedDaemonOptions,
    *,
    doctor: bool = False,
) -> dict[str, Any]:
    """Run only the documented daemon-switch control plane for the singleton."""
    command = [sys.executable, str(DAEMON_SWITCH), action, *options.command_arguments()]
    if action in {"quiesce", "restart"}:
        command.append("--yes")
    if doctor:
        command.append("--doctor")
    result = _public("command_result", command_result)(command, timeout=MANAGED_DAEMON_TIMEOUT_SECONDS)
    if result["exit_code"] != 0:
        detail = result["stderr"].strip() or result["stdout"].strip()
        raise SmokeError(f"daemon-switch {action} failed: {detail}")
    if action != "status":
        return {}
    try:
        status = json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise SmokeError(f"daemon-switch status returned invalid JSON: {error}") from error
    if not isinstance(status, dict):
        raise SmokeError("daemon-switch status returned a non-object response")
    if doctor:
        _public("require_ready_managed_doctor", require_ready_managed_doctor)(status)
    return status


def require_ready_managed_doctor(status: dict[str, Any]) -> None:
    """Accept only a healthy doctor paired to the selected daemon executable."""
    doctor_status = status.get("doctor")
    if not isinstance(doctor_status, dict) or "error" in doctor_status:
        detail = doctor_status.get("error") if isinstance(doctor_status, dict) else "missing doctor result"
        raise SmokeError(f"managed daemon doctor failed: {detail}")
    summary = doctor_status.get("summary")
    if not isinstance(summary, dict) or summary.get("status") != "healthy":
        raise SmokeError("managed daemon doctor is not healthy")
    runtime = doctor_status.get("runtime_status")
    if isinstance(runtime, dict) and runtime.get("readiness") != "ready":
        raise SmokeError("managed daemon doctor is not ready")
    live_pair = status.get("live_pair")
    if not isinstance(live_pair, dict) or live_pair.get("matched") is not True:
        detail = live_pair.get("detail") if isinstance(live_pair, dict) else "missing live-pair proof"
        raise SmokeError(f"managed daemon does not match the selected release: {detail}")
    client_context = doctor_status.get("client_context")
    daemon_context = doctor_status.get("daemon_context")
    client_version = client_context.get("version") if isinstance(client_context, dict) else None
    daemon_version = daemon_context.get("version") if isinstance(daemon_context, dict) else None
    if not isinstance(client_version, str) or not client_version:
        raise SmokeError("managed daemon doctor omitted the client version")
    if daemon_version is not None and client_version != daemon_version:
        raise SmokeError("managed daemon doctor reports mismatched client/daemon versions")
    selected_cli = status.get("atm")
    selected_version = selected_cli.get("version") if isinstance(selected_cli, dict) else None
    if not isinstance(selected_version, str) or not selected_version:
        raise SmokeError("daemon-switch status omitted the selected ATM CLI version")
    if selected_version.rsplit(maxsplit=1)[-1] != client_version:
        raise SmokeError("managed daemon doctor version differs from the selected ATM CLI")


def require_managed_peer_wire_security(status: dict[str, Any], expected: str) -> None:
    """Prove that the disposable managed daemon launched in the requested mode."""
    doctor_status = status.get("doctor")
    daemon_context = doctor_status.get("daemon_context") if isinstance(doctor_status, dict) else None
    actual = daemon_context.get("peer_wire_security") if isinstance(daemon_context, dict) else None
    if actual != expected:
        raise SmokeError(
            f"managed daemon peer-wire mode mismatch: expected {expected}, got {actual or '<missing>'}"
        )


def selected_pair(status: dict[str, Any]) -> dict[str, str | None]:
    """Keep only the selected-pair identity needed to prove no selector drift."""
    result: dict[str, str | None] = {}
    for role in ("atm", "atm_daemon"):
        value = status.get(role)
        if not isinstance(value, dict):
            raise SmokeError(f"daemon-switch status omitted {role}")
        for field in ("selector", "target"):
            item = value.get(field)
            if not isinstance(item, str) or not item:
                raise SmokeError(f"daemon-switch status omitted {role}.{field}")
            result[f"{role}.{field}"] = item
    return result


def resolved_managed_selector_links(
    options: ManagedDaemonOptions, status: dict[str, Any],
) -> ManagedDaemonOptions:
    """Pass discovered selector paths to daemon-switch under the scrubbed harness environment.

    The benchmark deliberately does not inherit the user's shell environment.
    Capture its already-proven selectors from ``daemon-switch status`` before
    quiescing the singleton, rather than depending on a Homebrew directory
    being present in the benchmark process's PATH.
    """
    pair = selected_pair(status)
    cli_link = options.cli_link or Path(str(pair["atm.selector"]))
    daemon_link = options.daemon_link or Path(str(pair["atm_daemon.selector"]))
    return replace(options, cli_link=cli_link, daemon_link=daemon_link)


@dataclass
class ManagedDaemonLifecycle:
    """Retired compatibility surface for the unsafe managed-host mode.

    It remains only to give callers a clear error. The runner never invokes a
    daemon-switch lifecycle while benchmarking.
    """

    options: ManagedDaemonOptions
    peer_wire_security: str | None = None
    managed_log_level: str | None = None
    launch_override: LaunchAgentPeerWireOverride | None = None

    def isolated_options(self) -> ManagedDaemonOptions:
        """Return the temporary launch configuration only for disposable state."""
        if self.peer_wire_security is None:
            return self.options
        if self.options.launch_agent_plist is None:
            raise SmokeError(
                "managed peer-wire benchmark requires --managed-launch-agent-plist on macOS"
            )
        if self.launch_override is None:
            self.launch_override = LaunchAgentPeerWireOverride.create(
                self.options.launch_agent_plist, self.peer_wire_security, self.managed_log_level,
            )
        return replace(self.options, launch_agent_plist=self.launch_override.override_path)

    def begin(self) -> None:
        raise SmokeError(
            "managed-daemon benchmark lifecycle is retired: it must not touch an ambient "
            "daemon or the primary OS-user-owned database"
        )



class DaemonOutputCapture:
    """Continuously drain daemon output and retain bounded diagnostic tails."""

    def __init__(self) -> None:
        self.ready_lines: Queue[str | None] = Queue()
        self._stdout_tail: list[str] = []
        self._stderr_tail: list[str] = []
        self._lock = Lock()
        self._threads: tuple[Thread, ...] = ()

    @classmethod
    def start(cls, process: subprocess.Popen[str]) -> "DaemonOutputCapture":
        if process.stdout is None or process.stderr is None:
            raise SmokeError("capacity daemon output streams were not captured")
        capture = cls()
        stdout = Thread(
            target=capture._drain_stdout,
            args=(process.stdout,),
            name="atm-capacity-stdout",
            daemon=True,
        )
        stderr = Thread(
            target=capture._drain_stderr,
            args=(process.stderr,),
            name="atm-capacity-stderr",
            daemon=True,
        )
        capture._threads = (stdout, stderr)
        stdout.start()
        stderr.start()
        return capture

    def _append_tail(self, destination: list[str], line: str) -> None:
        with self._lock:
            destination.append(line.rstrip("\n"))
            del destination[:-DAEMON_OUTPUT_TAIL_LINES]

    def _drain_stdout(self, stream: Any) -> None:
        try:
            for line in stream:
                self._append_tail(self._stdout_tail, line)
                self.ready_lines.put(line)
        finally:
            self.ready_lines.put(None)

    def _drain_stderr(self, stream: Any) -> None:
        for line in stream:
            self._append_tail(self._stderr_tail, line)

    def evidence(self) -> dict[str, list[str]]:
        with self._lock:
            return {
                "stdout_tail": list(self._stdout_tail),
                "stderr_tail": list(self._stderr_tail),
            }

    def join(self) -> None:
        for thread in self._threads:
            thread.join(timeout=1.0)


def require_capacity_benchmark_account() -> BenchmarkAccount:
    """Validate benchmark-account identity before any runner setup side effect."""
    if os.environ.get("ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE") == "1":
        raise SmokeError(
            "ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE is retired: benchmarks must not "
            "rename, replace, or restore the primary ~/.atm/db"
        )
    if os.environ.get("ATM_CAPACITY_ISOLATED_OS_USER"):
        raise SmokeError(
            "ATM_CAPACITY_ISOLATED_OS_USER is retired: a benchmark account must prove "
            "its account-local manifest rather than trust an environment assertion"
        )
    try:
        return _public("require_benchmark_account", require_benchmark_account)()
    except BenchmarkAccountError as error:
        raise SmokeError(f"benchmark-account preflight failed: {error}") from error


def reap_owned_daemon(process: subprocess.Popen[str]) -> None:
    """Terminate and reap the benchmark-owned child without mistaking a zombie for a leak."""
    _public("terminate_process", terminate_process)(process.pid)
    process.wait(timeout=10.0)


LIFECYCLE_RECOVERY = {
    "preflight": "correct the disposable benchmark-account manifest before retrying",
    "snapshot": "keep the benchmark daemon stopped and inspect retained snapshot staging material",
    "profile": "the runner will stop its owned daemon and restore the published clean snapshot",
    "restart": "keep the benchmark daemon stopped and inspect its restart diagnostics before retrying",
    "durability": "keep the benchmark daemon stopped and inspect the disposable benchmark store",
    "stop": "keep the benchmark daemon stopped; do not restore while SQLite sidecars may be active",
    "restore": "keep the benchmark daemon stopped and inspect retained restore staging material",
    "post_restore_verify": "keep the benchmark daemon stopped and inspect the restored benchmark account",
    "cleanup": "remove only the per-run temporary ATM_HOME after inspecting the retained evidence",
}


def verify_durability_after_restart(
    benchmark_account: BenchmarkAccount,
    roster: CapacityRoster,
    expected_accepted_count: int,
) -> dict[str, Any]:
    """Count this run's recipient rows after a fresh daemon has reopened SQLite.

    The benchmark roster is unique per target, so its recipient mailbox is an
    exact, isolated measurement: roster setup cannot contribute mail rows and
    no other target can share its ``(team, agent)`` pair.  The caller starts
    and doctors a new owned daemon immediately before this read; the query is
    deliberately read-only and never touches an interactive account.
    """
    if expected_accepted_count < 0:
        raise SmokeError("durability expected accepted count must not be negative")
    database = benchmark_account.durable_state_root / "mail.db"
    try:
        with closing(sqlite3.connect(f"file:{database}?mode=ro", uri=True)) as connection:
            row = connection.execute(
                "SELECT COUNT(*) FROM mail_messages WHERE team = ? AND agent = ?",
                (roster.team, roster.recipient),
            ).fetchone()
    except sqlite3.Error as error:
        raise SmokeError(f"could not count durable benchmark mailbox rows: {error}") from error
    if row is None or not isinstance(row[0], int):
        raise SmokeError("durability count query returned no integer mailbox count")
    observed = row[0]
    return {
        "expected_accepted_count": expected_accepted_count,
        "observed_mailbox_count": observed,
        "passed": observed == expected_accepted_count,
    }


def snapshot_evidence(snapshot: VerifiedSnapshot) -> dict[str, Any]:
    """Return non-sensitive facts from a verified account-local snapshot."""
    return {
        "snapshot_id": snapshot.snapshot_id,
        "account_identity": f"sha256:{hashlib.sha256(str(snapshot.account_id).encode()).hexdigest()[:16]}",
        "user_version": snapshot.user_version,
        "page_count": snapshot.page_count,
        "byte_count": snapshot.byte_count,
        "sha256": snapshot.sha256,
    }


def run_lifecycle_phase(
    evidence: dict[str, Any], phase: str, action: Callable[[], Any],
) -> Any:
    """Run one non-timed lifecycle action with durable diagnostic evidence."""
    if phase not in LIFECYCLE_RECOVERY:
        raise ValueError(f"unsupported benchmark lifecycle phase {phase!r}")
    started_wall = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    started = time.monotonic()
    try:
        result = action()
    except (
        BenchmarkMtlsError, BenchmarkSnapshotError, OSError, RuntimeError, ValueError, SmokeError,
        subprocess.TimeoutExpired,
    ) as error:
        finished_wall = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
        evidence.setdefault("lifecycle", {}).setdefault(phase, []).append(
            {
                "status": "failed",
                "started_at": started_wall,
                "finished_at": finished_wall,
                "duration_s": time.monotonic() - started,
                "cause": str(error),
                "recovery": LIFECYCLE_RECOVERY[phase],
            }
        )
        raise SmokeError(
            f"benchmark {phase} phase failed: {error}; recovery: {LIFECYCLE_RECOVERY[phase]}"
        ) from error
    finished_wall = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    evidence.setdefault("lifecycle", {}).setdefault(phase, []).append(
        {
            "status": "passed",
            "started_at": started_wall,
            "finished_at": finished_wall,
            "duration_s": time.monotonic() - started,
        }
    )
    return result


def os_account_home() -> Path:
    """Match ADR-026's OS-account root instead of trusting a shell HOME override."""
    if os.name == "nt":
        profile = os.environ.get("USERPROFILE", "").strip()
        if not profile:
            raise SmokeError("could not resolve the Windows OS-user profile for capacity smoke")
        return Path(profile)
    return Path(pwd.getpwuid(os.geteuid()).pw_dir)


def validate_capacity_home(path: Path) -> Path:
    """Accept only a fresh, clearly disposable temporary config directory."""
    if not path.is_absolute():
        raise SmokeError("capacity ATM_HOME must be an absolute path")
    resolved = path.resolve()
    if resolved == _public("os_account_home", os_account_home)().resolve() / ".atm":
        raise SmokeError("capacity runner must never target the production ~/.atm directory")
    temporary = Path(tempfile.gettempdir()).resolve()
    try:
        resolved.relative_to(temporary)
    except ValueError as error:
        raise SmokeError("capacity ATM_HOME must be below the OS temporary directory") from error
    if not resolved.name.startswith(CAPACITY_ROOT_PREFIX):
        raise SmokeError(f"capacity ATM_HOME basename must start with {CAPACITY_ROOT_PREFIX!r}")
    return resolved


def release_binary(name: str) -> Path:
    """Locate an already-built branch release executable without using PATH."""
    suffix = ".exe" if os.name == "nt" else ""
    path = ROOT / "target" / "release" / f"{name}{suffix}"
    if not path.is_file():
        raise SmokeError(f"release-built {name} is required at {path}; run cargo build --release first")
    return path


def sqlite_writer_probe() -> Path:
    """Build the existing direct async-writer probe when sqlite needs it.

    The SQLite target measures the existing Tokio async durable-admission
    seam: the bounded writer ingress, one-millisecond coalescing window, and
    committed SQLite transaction.  It deliberately excludes HTTP framing,
    request parsing, and canonical-send preparation, which are measured by
    the UDS/TCP/TCP+TLS targets and the explicit canonical-core diagnostic.
    """
    suffix = ".exe" if os.name == "nt" else ""
    probe = ROOT / "target" / "release" / f"atm-daemon-benchmark{suffix}"
    try:
        result = subprocess.run(
            [
                "cargo", "build", "--release", "-p", "atm-daemon-bootstrap",
                "--features", "benchmark-harness", "--bin", "atm-daemon-benchmark",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
            timeout=600.0,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SmokeError(f"could not build canonical sqlite writer probe: {error}") from error
    if result.returncode != 0 or not probe.is_file():
        detail = result.stderr.strip() or result.stdout.strip() or "no executable produced"
        raise SmokeError(f"could not build canonical sqlite writer probe: {detail}")
    return probe


def source_revision() -> str:
    """Bind retained benchmark evidence to the checkout that built the daemon."""
    revision = _git_source_revision(ROOT)
    if revision is None:
        raise SmokeError("capacity benchmark requires a Git checkout with a resolved HEAD revision")
    return revision


def release_version(binary: Path) -> str:
    """Capture the selected release binary version for public provenance."""
    result = command_result([str(binary), "--version"], timeout=10.0)
    version = result["stdout"].strip()
    if result["exit_code"] != 0 or not version:
        detail = result["stderr"].strip() or version or "no version output"
        raise SmokeError(f"could not read benchmark client version: {detail}")
    return version


def is_ancestor_revision(candidate: str, current: str) -> bool:
    """Return whether an accepted evidence revision is in this checkout's history."""
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", candidate, current],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0


def runtime_environment(
    atm_home: Path, roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> dict[str, str]:
    environment = dict(os.environ)
    environment.update(
        {
            "ATM_HOME": str(atm_home),
            "ATM_IDENTITY": roster.agent,
            "ATM_TEAM": roster.team,
            "ATM_CAPACITY_RUN_ID": roster.run_id,
            "ATM_DAEMON_READY_STDOUT": "1",
        }
    )
    return environment


def allocate_direct_peer_port() -> int:
    """Select an unoccupied loopback port for one benchmark daemon launch.

    A physical benchmark account may share its host with a live daemon, whose
    durable direct-peer listener uses the protocol default port.  The harness
    must therefore select its own explicit listener rather than silently
    continue after a port-bind failure and exercise another account's daemon.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    if port == DIRECT_PEER_TCP_PORT:
        raise SmokeError("benchmark direct-peer port collided with the protocol default")
    return port


def benchmark_runtime_client_environment(environment: dict[str, str]) -> dict[str, str]:
    """Keep doctor bound to the disposable daemon that this runner launched.

    The benchmark daemon receives its own ``ATM_HOME``.  Its public doctor
    probe must use that same runtime selection; removing it instead queries an
    unrelated ambient account daemon and can turn a healthy benchmark daemon
    into a spurious warning result.
    """
    return dict(environment)


def benchmark_doctor_payload(result: dict[str, object]) -> dict[str, object]:
    """Validate the benchmark daemon's ready state from its public doctor response.

    Capacity runs exercise the same shipped Tokio/Axum daemon used by normal
    traffic.  Its observability and received-message hook therefore remain
    active and doctor must be completely healthy; a harness-only exception
    would conceal a real deployment defect.
    """
    stdout = result.get("stdout")
    if not isinstance(stdout, str):
        raise SmokeError("capacity doctor returned no JSON response")
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise SmokeError("capacity doctor returned malformed JSON") from error
    if not isinstance(payload, dict):
        raise SmokeError("capacity doctor response must be an object")

    runtime_status = payload.get("runtime_status")
    if not isinstance(runtime_status, dict):
        raise SmokeError("capacity doctor did not report runtime status")
    if runtime_status.get("liveness") != "running" or runtime_status.get("readiness") != "ready":
        raise SmokeError("capacity doctor did not report a running, ready runtime")

    summary = payload.get("summary")
    if not isinstance(summary, dict):
        raise SmokeError("capacity doctor did not report a summary")
    if summary.get("status") == "healthy" and result.get("exit_code") == 0:
        return payload

    detail = result.get("stderr")
    if not isinstance(detail, str) or not detail.strip():
        detail = f"summary status {summary.get('status')!r}"
    raise SmokeError(f"capacity doctor failed: {detail.strip()}")


def await_daemon_ready(process: subprocess.Popen[str], output: DaemonOutputCapture) -> None:
    """Wait only for the daemon's explicit readiness signal."""
    deadline = time.monotonic() + READY_TIMEOUT_SECONDS
    last_line = ""
    while time.monotonic() < deadline:
        try:
            line = output.ready_lines.get(
                timeout=min(0.1, max(0.0, deadline - time.monotonic()))
            )
        except Empty:
            if process.poll() is not None:
                raise SmokeError(f"capacity daemon exited before ready: {last_line.strip()}")
            continue
        if line is None:
            raise SmokeError(f"capacity daemon exited before ready: {last_line.strip()}")
        last_line = line
        if line.strip() == "ATM_DAEMON_READY":
            return
    raise SmokeError("capacity daemon did not publish ATM_DAEMON_READY within 30 seconds")


def start_capacity_daemon(
    daemon: Path,
    home: Path,
    env: dict[str, str],
    peer_wire_security: str = "mutual-tls",
) -> tuple[subprocess.Popen[str], DaemonOutputCapture]:
    """Start the shipped daemon with its ordinary explicit peer-wire mode."""
    peer_wire_security = validate_peer_wire_security(peer_wire_security)
    _public("require_clean_host_daemon_state", require_clean_host_daemon_state)(
        smoke_label="admission-capacity daemon launch"
    )
    command = [str(daemon), "--peer-wire-security", peer_wire_security]
    if direct_peer_port := env.get(CAPACITY_DIRECT_PEER_PORT_ENV):
        command.extend(("--direct-peer-port", direct_peer_port))
    process = subprocess.Popen(
        command,
        cwd=home,
        env=env,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    output = _public("DaemonOutputCapture", DaemonOutputCapture).start(process)
    try:
        _public("await_daemon_ready", await_daemon_ready)(process, output)
    except (OSError, SmokeError) as error:
        _public("reap_owned_daemon", reap_owned_daemon)(process)
        output.join()
        tails = output.evidence()
        if isinstance(tails, dict):
            stdout_lines = tails.get("stdout_tail", [])
            stderr_lines = tails.get("stderr_tail", [])
        else:
            stdout_lines, stderr_lines = [], []
        stdout_tail = " | ".join(str(line) for line in stdout_lines[-8:]) or "<unavailable>"
        stderr_tail = " | ".join(str(line) for line in stderr_lines[-8:]) or "<unavailable>"
        raise SmokeError(
            f"{error}; daemon stdout tail: {stdout_tail}; daemon stderr tail: {stderr_tail}"
        ) from error
    return process, output


def prepare_capacity_roster(
    atm: Path,
    env: dict[str, str],
    home: Path,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> None:
    """Create the disposable roster used exclusively by public benchmark writes."""
    for team, member, agent_type in (
        (roster.team, roster.agent, "lead"),
        (roster.team, roster.recipient, "general-purpose"),
    ):
        result = _public("command_result", command_result)(
            [
                str(atm), "teams", "add-member", team, member,
                "--agent-type", agent_type, "--home-dir", str(home), "--json",
            ],
            timeout=15.0,
            env=env,
        )
        if result["exit_code"] != 0:
            raise SmokeError(
                f"could not create capacity roster member {member}: {result['stderr'].strip()}"
            )


def http_request_body(
    home: Path,
    sequence: int,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
    *,
    peer_origin: bool = False,
) -> bytes:
    """Build the documented /v1/atm/messages request; no dispatcher shortcut."""
    payload = {
        "home_dir": str(home),
        "current_dir": str(home),
        "caller_identity": roster.agent,
        "caller_team": roster.team,
        "to": {"agent": roster.recipient, "team": roster.team},
        "message_source": {"Inline": f"capacity-{sequence}"},
        "summary_override": None,
        "requires_ack": False,
        "task_id": None,
        "parent_message_id": None,
        "thread_mode": None,
        "expires_at": None,
        "dry_run": False,
    }
    if peer_origin:
        payload.update(benchmark_origin_metadata(sequence))
    return json.dumps(payload, separators=(",", ":")).encode("utf-8")


def benchmark_origin_metadata(sequence: int) -> dict[str, str]:
    """Return the immutable provenance pair required by direct-peer ingress."""
    milliseconds = int(time.time() * 1_000)
    entropy = (uuid.uuid4().int ^ sequence) & ((1 << 80) - 1)
    value = (milliseconds << 80) | entropy
    encoded = "".join(
        CROCKFORD_BASE32[(value >> (5 * offset)) & 0x1F]
        for offset in range(25, -1, -1)
    )
    timestamp = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    return {"origin_message_id": encoded, "origin_timestamp": timestamp}


def cached_roster_heartbeat_body(
    sequence: int,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> bytes:
    """Build a heartbeat that validates against the warmed roster snapshot.

    The daemon's heartbeat route calls ``LocalServiceRuntime.load_roster_member``.
    That method reads SQLite only for the first request of a team and serves its
    immutable in-process snapshot thereafter.  The benchmark explicitly warms
    that first request before recording these samples.
    """
    payload = {
        "team": roster.team,
        "member": roster.agent,
        "pid": 90_000 + sequence,
        "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "activity": "active_tool_use",
    }
    return json.dumps(payload, separators=(",", ":")).encode("utf-8")


def validate_transport(transport: str) -> str:
    """Keep platform transport selection explicit and comparable."""
    if transport not in {"sqlite", "uds", "tcp"}:
        raise SmokeError("capacity transport must be `sqlite`, `uds`, or `tcp`")
    if os.name == "nt" and transport == "uds":
        raise SmokeError("Windows capacity benchmarking does not support UDS")
    return transport


def validate_peer_wire_security(value: str) -> str:
    """Accept only the daemon's public, launch-owned peer-wire values."""
    if value not in PEER_WIRE_SECURITY_MODES:
        raise SmokeError(
            "capacity peer-wire security must be `mutual-tls` or `plaintext-test`"
        )
    return value


def resolve_benchmark_target(
    target: str | None,
    transport: str | None,
) -> tuple[str, str | None, str | None]:
    """Resolve public targets without inventing a benchmark-only transport.

    `tcp` deliberately selects the existing direct plaintext pipeline and
    `tcp-tls` selects the same direct-peer listener with its ordinary mTLS
    stream wrapper.  SQLite and UDS are local boundaries, so their target map
    never asks the daemon to construct an mTLS adapter.  Legacy
    ``--transport`` remains diagnostic-only and defaults to plaintext because
    it has no target-level peer-wire claim.
    """
    if target is not None:
        selected_transport, peer_wire_security = BENCHMARK_TARGETS[target]
        if transport is not None and transport != selected_transport:
            raise SmokeError(
                f"benchmark target {target!r} requires transport {selected_transport!r}"
            )
        return selected_transport, peer_wire_security, target
    return validate_transport(transport or ("uds" if os.name != "nt" else "tcp")), "plaintext-test", None


def local_endpoint(transport: str) -> LocalEndpoint:
    """Resolve an ordinary same-host UDS endpoint without a dispatcher seam."""
    runtime = os_account_home() / ".atm" / "daemon"
    if transport == "sqlite":
        raise SmokeError("sqlite capacity target has no public socket endpoint")
    if transport == "uds":
        return LocalEndpoint("uds", str(runtime / "atm-daemon.sock"))
    raise SmokeError("TCP benchmark targets must use the direct-peer listener, not local-http.json")


def direct_peer_endpoint(
    port: int,
    identity: DisposableMtlsIdentity | None = None,
) -> LocalEndpoint:
    """Return the fixed direct-peer listener rather than the local HTTP record.

    The direct listener is deliberately separate from the capability-authenticated
    loopback listener.  Pinning this address makes a TCP result prove the
    DirectPeerTcpConfig path (and, when supplied, the mTLS stream adapter).
    """
    return LocalEndpoint(
        "tcp",
        ("127.0.0.1", port),
        direct_peer=True,
        tls_server_name=identity.host if identity is not None else None,
        tls_certificate_bundle=identity.certificate_bundle if identity is not None else None,
    )


def disposable_peer_host(roster: CapacityRoster) -> str:
    """Resolve one durable authority for a local disposable mTLS peer.

    A literal address is intentionally forbidden: the same durable hostname is
    stored in peer configuration, checked by the TLS client, and used by the
    server-side pin map.  An operator may supply a host only through the
    explicit benchmark variable; no machine name is embedded in the runner.
    """
    configured = os.environ.get("ATM_CAPACITY_PEER_HOST")
    host = (configured if configured is not None else socket.getfqdn()).strip().rstrip(".")
    # Reverse-DNS on some hosts yields an ephemeral, oversized .arpa artifact.
    # Prefer the durable local hostname before asking the operator to configure one.
    invalid = host.endswith(".arpa") or len(host) > 64 or "." not in host or host.startswith(".")
    if invalid and configured is None:
        fallback = socket.gethostname().strip().rstrip(".")
        if fallback and not fallback.endswith(".arpa") and len(fallback) <= 64 and "." in fallback:
            host = fallback
    try:
        socket.inet_pton(socket.AF_INET, host)
        raise SmokeError("benchmark mTLS authority must be a durable hostname, not an IPv4 address")
    except OSError:
        pass
    authority = f"capacity-{roster.run_id}.{host}"
    if "." not in host or host.startswith(".") or host.endswith(".") or host.endswith(".arpa") or len(host) > 64 or len(authority) > 64:
        raise SmokeError(
            "benchmark mTLS authority must be a durable DNS or mDNS hostname; "
            "set ATM_CAPACITY_PEER_HOST explicitly when local DNS is incomplete"
        )
    return authority


def _required_command(command: list[str], env: dict[str, str], description: str) -> None:
    result = command_result(command, timeout=20.0, env=env)
    if result["exit_code"] == 0:
        return
    detail = result["stderr"].strip() or result["stdout"].strip() or "no diagnostic output"
    raise SmokeError(f"could not {description}: {detail}")


def provision_disposable_mtls_identity(
    atm: Path,
    env: dict[str, str],
    home: Path,
    roster: CapacityRoster,
    direct_peer_port: int,
) -> DisposableMtlsIdentity:
    """Install a self-contained, short-lived peer configuration before launch.

    This is benchmark control-plane setup, not a transport shortcut: the
    daemon reads the normal persisted interface/certificate/trust records at
    startup and the client validates the same certificate and hostname over
    the fixed direct-peer listener.
    """
    openssl = shutil.which("openssl")
    if openssl is None:
        raise SmokeError("mTLS benchmark setup requires the system `openssl` executable")
    host = disposable_peer_host(roster)
    tls_directory = home / "benchmark-mtls"
    tls_directory.mkdir(mode=0o700)
    config = tls_directory / "openssl.cnf"
    private_key = tls_directory / "identity.key"
    certificate = tls_directory / "identity.crt"
    bundle = tls_directory / "identity.pem"
    config.write_text(
        "[req]\n"
        "prompt = no\n"
        "distinguished_name = subject\n"
        "x509_extensions = extensions\n"
        "[subject]\n"
        f"CN = {host}\n"
        "[extensions]\n"
        f"subjectAltName = DNS:{host}\n"
        # This short-lived self-signed certificate is the benchmark's local
        # trust anchor as well as its mTLS identity.  CA:TRUE is required for
        # standard TLS verification to accept that explicit local anchor; it
        # is retained only inside the disposable benchmark account.
        "basicConstraints = critical,CA:TRUE\n"
        "keyUsage = critical,digitalSignature,keyEncipherment,keyCertSign\n"
        "extendedKeyUsage = serverAuth,clientAuth\n",
        encoding="utf-8",
    )
    _required_command(
        [
            # The disposable benchmark identity is P-256, the standard
            # modern TLS signing key.  Explicit named-curve encoding matters:
            # macOS' LibreSSL otherwise serializes explicit EC parameters that
            # Rustls deliberately rejects.
            openssl, "genpkey", "-algorithm", "EC", "-pkeyopt",
            "ec_paramgen_curve:prime256v1", "-pkeyopt", "ec_param_enc:named_curve",
            "-out", str(private_key),
        ],
        env,
        "generate disposable P-256 mTLS private key",
    )
    _required_command(
        [
            openssl, "req", "-x509", "-new", "-key", str(private_key),
            "-out", str(certificate), "-days", "2",
            "-config", str(config),
        ],
        env,
        "generate disposable P-256 mTLS certificate",
    )
    bundle.write_bytes(certificate.read_bytes() + private_key.read_bytes())
    try:
        der = ssl.PEM_cert_to_DER_cert(certificate.read_text(encoding="ascii"))
    except (OSError, ValueError) as error:
        raise SmokeError(f"could not calculate disposable mTLS certificate fingerprint: {error}") from error
    fingerprint = hashlib.sha256(der).hexdigest()
    _required_command(
        [
            str(atm), "peer", "interface", "set", "--bind", f"127.0.0.1:{direct_peer_port}",
            "--advertise-host", host, "--enabled",
        ],
        env,
        "save disposable mTLS interface",
    )
    _required_command(
        [
            str(atm), "peer", "certificate", "init", "--fingerprint", fingerprint,
            "--private-key-ref", str(bundle), "--yes",
        ],
        env,
        "save disposable mTLS local identity",
    )
    _required_command(
        [
            str(atm), "peer", "trust", "add", "--host", host,
            "--fingerprint", fingerprint, "--https-port", str(direct_peer_port), "--yes",
        ],
        env,
        "save disposable mTLS trusted peer",
    )
    return DisposableMtlsIdentity(host, bundle, fingerprint)


def read_http_response(
    stream: socket.socket,
    buffered: bytearray | None = None,
) -> tuple[int, int, str | None]:
    """Consume one complete HTTP response and retain a bounded error summary."""
    data = buffered if buffered is not None else bytearray()
    while b"\r\n\r\n" not in data:
        chunk = stream.recv(4096)
        if not chunk:
            raise SmokeError("daemon closed the local HTTP connection before response headers")
        data.extend(chunk)
        if len(data) > 16_384:
            raise SmokeError("daemon local HTTP response headers exceeded the safety bound")
    header_end = data.index(b"\r\n\r\n") + 4
    status_line = bytes(data[:header_end]).split(b"\r\n", 1)[0].decode("ascii", "replace")
    fields = status_line.split()
    if len(fields) < 2 or not fields[1].isdigit():
        raise SmokeError(f"daemon returned malformed HTTP status line: {status_line}")
    content_length = 0
    for line in bytes(data[:header_end]).split(b"\r\n")[1:]:
        name, separator, value = line.partition(b":")
        if separator and name.lower() == b"content-length":
            content_length = int(value.strip())
            break
    while len(data) < header_end + content_length:
        chunk = stream.recv(min(4096, header_end + content_length - len(data)))
        if not chunk:
            raise SmokeError("daemon closed the local HTTP connection before its declared response body")
        data.extend(chunk)
    status = int(fields[1])
    frame_end = header_end + content_length
    body = bytes(data[header_end:frame_end])
    del data[:frame_end]
    summary = body[:512].decode("utf-8", "replace") if status >= 400 else None
    return status, header_end + content_length, summary


def tls_client_context(endpoint: LocalEndpoint) -> ssl.SSLContext | None:
    """Prepare one immutable mTLS client context for a benchmark profile.

    A profile represents one benchmark client.  Reusing its context is both
    representative of a long-lived client and lets TLS safely reuse sessions
    across the profile's short-lived HTTP connections.  Context construction
    and PEM parsing are setup work, so they must not be charged to each timed
    connection.
    """
    if endpoint.tls_server_name is None:
        return None
    if endpoint.tls_certificate_bundle is None:
        raise SmokeError("direct mTLS endpoint omitted its benchmark identity bundle")
    context = ssl.create_default_context(
        ssl.Purpose.SERVER_AUTH,
        cafile=str(endpoint.tls_certificate_bundle),
    )
    context.load_cert_chain(str(endpoint.tls_certificate_bundle))
    return context


def submit_connection(
    endpoint: LocalEndpoint,
    requests: list[HttpRequest],
    tls_context: ssl.SSLContext | None = None,
) -> list[AdmissionResult]:
    """Submit consecutive real requests over one selected public connection."""
    started = time.perf_counter()
    capability = (
        f"X-ATM-Local-Capability: {endpoint.capability}\r\n".encode("ascii")
        if endpoint.capability is not None
        else b""
    )
    results: list[AdmissionResult] = []
    try:
        family = socket.AF_UNIX if endpoint.kind == "uds" else socket.AF_INET
        with socket.socket(family, socket.SOCK_STREAM) as raw_stream:
            raw_stream.settimeout(3.5)
            if endpoint.kind == "tcp":
                raw_stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            raw_stream.connect(endpoint.address)
            if endpoint.tls_server_name is not None:
                context = tls_context or tls_client_context(endpoint)
                if context is None:
                    raise SmokeError("direct mTLS endpoint omitted its benchmark identity bundle")
                stream = context.wrap_socket(
                    raw_stream,
                    server_hostname=endpoint.tls_server_name,
                )
            else:
                stream = raw_stream
            frames = []
            if endpoint.kind == "tcp":
                host = endpoint.tls_server_name or str(endpoint.address[0])
                authority = f"Host: {host}:{endpoint.address[1]}\r\n".encode("ascii")
            else:
                authority = b"Host: localhost\r\n"
            for index, request in enumerate(requests):
                connection = "close" if index + 1 == len(requests) else "keep-alive"
                frames.append(
                    f"POST {request.path} HTTP/1.1\r\n".encode("ascii")
                    + b"Content-Type: application/json\r\n"
                    + authority
                    + capability
                    + f"Content-Length: {len(request.body)}\r\nConnection: {connection}\r\n\r\n".encode("ascii")
                    + request.body
                )

            response_buffer = bytearray()
            for start in range(0, len(frames), MAX_IN_FLIGHT_REQUESTS):
                batch = frames[start:start + MAX_IN_FLIGHT_REQUESTS]
                request_started = time.perf_counter()
                stream.sendall(b"".join(batch))
                for request, frame in zip(requests[start:start + MAX_IN_FLIGHT_REQUESTS], batch):
                    status, response_bytes, response_summary = read_http_response(stream, response_buffer)
                    results.append(AdmissionResult(
                        status=status,
                        elapsed_ms=(time.perf_counter() - request_started) * 1_000,
                        failure=(
                            None
                            if status == request.expected_status
                            else f"HTTP {status}: {response_summary or 'no response body'}"
                        ),
                        request_bytes=len(frame),
                        response_bytes=response_bytes,
                        response_summary=response_summary,
                    ))
            if stream is not raw_stream:
                stream.close()
        return results
    except (OSError, SmokeError) as error:
        return results + [AdmissionResult(
            status=0, elapsed_ms=(time.perf_counter() - started) * 1_000,
            failure=str(error),
        )]


def admission_connection_worker_limit(requested_workers: int) -> int:
    """Bound concurrent client sockets by the process soft descriptor limit.

    This is deliberately a benchmark-client concern. It does not tune or
    modify the daemon, its transport, or the production write pipeline.
    """
    if requested_workers <= 0:
        raise SmokeError("capacity workers must be positive")
    active_resource = _public("resource", resource)
    if active_resource is None:
        return requested_workers
    try:
        soft_limit, _hard_limit = active_resource.getrlimit(active_resource.RLIMIT_NOFILE)
    except (OSError, ValueError):
        return requested_workers
    if soft_limit == active_resource.RLIM_INFINITY:
        return requested_workers
    reserve = _public("DESCRIPTOR_RESERVE", DESCRIPTOR_RESERVE)
    return min(requested_workers, max(1, int(soft_limit) - reserve))


def run_interval(
    submit: Callable[[int, int], list[AdmissionResult]],
    interval: int,
    frames_per_connection: int,
    workers: int,
    requested_messages: int = ADMISSIONS_PER_INTERVAL,
    expected_status: int = 201,
) -> dict[str, Any]:
    """Run one exactly-sized admission interval without retrying failed writes."""
    if requested_messages <= 0:
        raise SmokeError("requested messages must be positive")
    started = time.perf_counter()
    results: list[AdmissionResult] = []
    connections = (requested_messages + frames_per_connection - 1) // frames_per_connection
    connection_workers = min(connections, admission_connection_worker_limit(workers))
    with ThreadPoolExecutor(max_workers=connection_workers, thread_name_prefix="atm-capacity") as executor:
        futures = [
            executor.submit(
                submit,
                interval * requested_messages + sequence * frames_per_connection,
                min(frames_per_connection, requested_messages - sequence * frames_per_connection),
            )
            for sequence in range(connections)
        ]
        for future in as_completed(futures):
            results.extend(future.result())
    elapsed_seconds = time.perf_counter() - started
    accepted = sum(result.status == expected_status for result in results)
    failures = [
        result.failure or f"HTTP {result.status}"
        for result in results
        if result.status != expected_status
    ]
    latencies = [result.elapsed_ms for result in results]
    latency_distribution = distribution(latencies) if latencies else {
        "min": 0.0,
        "p50": 0.0,
        "p95": 0.0,
        "p99": 0.0,
        "max": 0.0,
    }
    error_free = accepted == requested_messages and not failures
    request_bytes = sum(result.request_bytes for result in results)
    response_bytes = sum(result.response_bytes for result in results)
    return {
        "interval": interval + 1,
        "accepted_count": accepted,
        "response_count": len(results),
        "elapsed_seconds": elapsed_seconds,
        "admissions_per_second": accepted / elapsed_seconds if elapsed_seconds else 0.0,
        "latency_ms": latency_distribution,
        "connections": connections,
        "connection_workers": connection_workers,
        "request_frames_per_second": len(results) / elapsed_seconds if elapsed_seconds else 0.0,
        "connections_per_second": connections / elapsed_seconds if elapsed_seconds else 0.0,
        "requested_count": requested_messages,
        "time_to_send_1k_s": elapsed_seconds * (1_000 / max(accepted, 1)),
        "application_wire_bytes": {
            "request": request_bytes,
            "response": response_bytes,
            "total": request_bytes + response_bytes,
        },
        "application_wire_bytes_per_second": (
            (request_bytes + response_bytes) / elapsed_seconds if elapsed_seconds else 0.0
        ),
        "error_free": error_free,
        "bytes_per_second": (
            (request_bytes + response_bytes) / elapsed_seconds if elapsed_seconds else 0.0
        ),
        "first_failure": failures[0] if failures else None,
        # This is a factual request/response outcome.  The reviewed v4 floor
        # is applied exactly once by classify_status() when result evidence
        # is emitted.
        "passed": error_free,
    }



__all__ = (
    'AdmissionResult',
    'HttpRequest',
    'LocalEndpoint',
    'DisposableMtlsIdentity',
    'CapacityRoster',
    'CapacityRunResult',
    'V4EmissionContext',
    'DEFAULT_CAPACITY_ROSTER',
    'benchmark_os',
    'binary_hashes',
    'atomic_json',
    'v4_result_from_evidence',
    'write_v4_evidence',
    'HostStateBackup',
    'ManagedDaemonOptions',
    'LaunchAgentPeerWireOverride',
    'daemon_switch_result',
    'require_ready_managed_doctor',
    'require_managed_peer_wire_security',
    'selected_pair',
    'resolved_managed_selector_links',
    'ManagedDaemonLifecycle',
    'DaemonOutputCapture',
    'require_capacity_benchmark_account',
    'reap_owned_daemon',
    'verify_durability_after_restart',
    'snapshot_evidence',
    'run_lifecycle_phase',
    'os_account_home',
    'validate_capacity_home',
    'release_binary',
    'sqlite_writer_probe',
    'source_revision',
    'release_version',
    'is_ancestor_revision',
    'runtime_environment',
    'allocate_direct_peer_port',
    'benchmark_runtime_client_environment',
    'benchmark_doctor_payload',
    'await_daemon_ready',
    'start_capacity_daemon',
    'prepare_capacity_roster',
    'http_request_body',
    'benchmark_origin_metadata',
    'cached_roster_heartbeat_body',
    'validate_transport',
    'validate_peer_wire_security',
    'resolve_benchmark_target',
    'local_endpoint',
    'direct_peer_endpoint',
    'disposable_peer_host',
    'provision_disposable_mtls_identity',
    'read_http_response',
    'tls_client_context',
    'submit_connection',
    'admission_connection_worker_limit',
    'run_interval',
    'DAEMON_OUTPUT_TAIL_LINES',
 )

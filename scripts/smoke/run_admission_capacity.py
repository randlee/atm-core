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
from scripts.smoke.benchmark_mtls import BenchmarkMtlsError, regenerate_mtls_identity
from scripts.smoke.benchmark_snapshot import (
    BenchmarkSnapshotError,
    VerifiedSnapshot,
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
INTERVALS = 10
ADMISSIONS_PER_INTERVAL = 1_000
TARGET_PROFILE_DURATION_SECONDS = 20.0
DEFAULT_WORKERS = 64
# Keep the benchmark's HTTP/1.1 pipeline below the local socket's bidirectional
# buffer capacity. The sender writes one bounded batch, then the reader drains
# every matching response before the next batch.
MAX_IN_FLIGHT_REQUESTS = 8
READY_TIMEOUT_SECONDS = 30.0
CAPACITY_ROOT_PREFIX = "atm-capacity-"
SPARSE_FRAMES_PER_CONNECTION = (1, 2, 4, 8, 16, 64)
SUSTAINED_MESSAGE_COUNTS = (10_000, 100_000)
DAEMON_OUTPUT_TAIL_LINES = 200
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")
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


def load_baselines(path: Path = BASELINES_PATH) -> BaselineSet:
    """Load the single reviewed source of per-host/per-target acceptance floors."""
    try:
        return BaselineSet.model_validate_json(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise SmokeError(f"could not load benchmark baselines {path}: {error}") from error


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
    return BenchmarkRunResult(
        campaign_id=context.campaign_id,
        host_label=str(evidence["host_label"]),
        os=context.os_name,
        target=context.target,
        status=status,
        incomplete_reason=(
            None
            if complete
            else str(evidence.get("failure") or "benchmark did not complete every lifecycle stage")
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
        raise SmokeError(f"could not emit direct v4 benchmark evidence: {error}") from error
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
    result = command_result(command, timeout=MANAGED_DAEMON_TIMEOUT_SECONDS)
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
        require_ready_managed_doctor(status)
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
        return require_benchmark_account()
    except BenchmarkAccountError as error:
        raise SmokeError(f"benchmark-account preflight failed: {error}") from error


def reap_owned_daemon(process: subprocess.Popen[str]) -> None:
    """Terminate and reap the benchmark-owned child without mistaking a zombie for a leak."""
    terminate_process(process.pid)
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
    if resolved == os_account_home().resolve() / ".atm":
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


def canonical_writer_probe() -> Path:
    """Build the existing direct canonical-writer probe when sqlite needs it.

    The normal benchmark recipe still starts only the shipped Tokio/Axum
    daemon.  This feature-gated helper does not create a second daemon: its
    sole invocation is ``--direct-core-write``, which measures the existing
    async admission/writer/commit path without an HTTP codec.
    """
    suffix = ".exe" if os.name == "nt" else ""
    probe = ROOT / "target" / "release" / f"atm-daemon-benchmark{suffix}"
    if probe.is_file():
        return probe
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
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True,
        text=True, check=False,
    )
    revision = result.stdout.strip()
    if result.returncode != 0 or not GIT_REVISION.fullmatch(revision):
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


def host_runtime_client_environment(environment: dict[str, str]) -> dict[str, str]:
    """Use the OS-user runtime record, never the disposable config root, for doctor."""
    result = dict(environment)
    result.pop("ATM_HOME", None)
    return result


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
    direct_peer_port = env.get(CAPACITY_DIRECT_PEER_PORT_ENV)
    command = [str(daemon), "--peer-wire-security", peer_wire_security]
    if direct_peer_port is not None:
        command.extend(("--direct-peer-port", direct_peer_port))
    process = subprocess.Popen(
        command,
        cwd=home,
        env=env,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    output = DaemonOutputCapture.start(process)
    try:
        await_daemon_ready(process, output)
    except (OSError, SmokeError) as error:
        reap_owned_daemon(process)
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
    for team, member in (
        (roster.team, roster.agent),
        (roster.team, roster.recipient),
    ):
        result = command_result(
            [str(atm), "teams", "add-member", team, member, "--home-dir", str(home), "--json"],
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
    host = os.environ.get("ATM_CAPACITY_PEER_HOST", socket.getfqdn()).strip().rstrip(".")
    try:
        socket.inet_pton(socket.AF_INET, host)
        raise SmokeError("benchmark mTLS authority must be a durable hostname, not an IPv4 address")
    except OSError:
        pass
    if "." not in host or host.startswith(".") or host.endswith("."):
        raise SmokeError(
            "benchmark mTLS authority must be a durable DNS or mDNS hostname; "
            "set ATM_CAPACITY_PEER_HOST explicitly when local DNS is incomplete"
        )
    return f"capacity-{roster.run_id}.{host}"


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
            openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(private_key), "-out", str(certificate), "-days", "2",
            "-config", str(config),
        ],
        env,
        "generate disposable mTLS identity",
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


def submit_connection(endpoint: LocalEndpoint, requests: list[HttpRequest]) -> list[AdmissionResult]:
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
                if endpoint.tls_certificate_bundle is None:
                    raise SmokeError("direct mTLS endpoint omitted its benchmark identity bundle")
                context = ssl.create_default_context(
                    ssl.Purpose.SERVER_AUTH,
                    cafile=str(endpoint.tls_certificate_bundle),
                )
                context.load_cert_chain(str(endpoint.tls_certificate_bundle))
                stream = context.wrap_socket(raw_stream, server_hostname=endpoint.tls_server_name)
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
    if resource is None:
        return requested_workers
    try:
        soft_limit, _hard_limit = resource.getrlimit(resource.RLIMIT_NOFILE)
    except (OSError, ValueError):
        return requested_workers
    if soft_limit == resource.RLIM_INFINITY:
        return requested_workers
    return min(requested_workers, max(1, int(soft_limit) - DESCRIPTOR_RESERVE))


def run_interval(
    submit: Callable[[int, int], list[AdmissionResult]],
    interval: int,
    frames_per_connection: int,
    workers: int,
    requested_messages: int = ADMISSIONS_PER_INTERVAL,
    expected_status: int = 201,
    minimum_admissions_per_second: float = 1_000.0,
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
        "passed": error_free and (
            minimum_admissions_per_second <= 0
            or elapsed_seconds <= requested_messages / minimum_admissions_per_second
        ),
    }


def run_profile(
    endpoint: LocalEndpoint,
    home: Path,
    frames_per_connection: int,
    requested_messages: int,
    sample_count: int,
    workers: int,
    target_duration_seconds: float = TARGET_PROFILE_DURATION_SECONDS,
    operation: str = "write",
    expected_status: int = 201,
    minimum_admissions_per_second: float = 1_000.0,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> dict[str, Any]:
    """Collect at least ten independent intervals over one sustained profile."""
    if sample_count <= 0:
        raise SmokeError("capacity sample count must be positive")
    if target_duration_seconds <= 0:
        raise SmokeError("capacity target duration must be positive")

    def submit(sequence: int, message_count: int) -> list[AdmissionResult]:
        if operation == "write":
            requests = [
                HttpRequest(
                    "/v1/atm/messages",
                    http_request_body(
                        home,
                        sequence + offset,
                        roster,
                        peer_origin=endpoint.direct_peer,
                    ),
                    201,
                )
                for offset in range(message_count)
            ]
        elif operation == "cached_roster_heartbeat":
            requests = [
                HttpRequest(
                    "/v1/atm/heartbeat",
                    cached_roster_heartbeat_body(sequence + offset, roster),
                    200,
                )
                for offset in range(message_count)
            ]
        else:
            raise SmokeError(f"unsupported capacity benchmark operation {operation!r}")
        return submit_connection(endpoint, requests)

    intervals: list[dict[str, Any]] = []
    elapsed_seconds = 0.0
    while len(intervals) < sample_count or elapsed_seconds < target_duration_seconds:
        interval = run_interval(
            submit,
            len(intervals),
            frames_per_connection,
            workers,
            requested_messages,
            expected_status=expected_status,
            minimum_admissions_per_second=minimum_admissions_per_second,
        )
        intervals.append(interval)
        elapsed_seconds += float(interval["elapsed_seconds"])
        # Keep clean under-threshold intervals to distinguish a throughput
        # plateau from an actual request/response failure.
        if not interval.get("error_free", interval["passed"]):
            break
    return {
        "operation": operation,
        "recipient": f"{roster.recipient}@{roster.team}",
        "requested_messages_per_sample": requested_messages,
        "minimum_sample_count": sample_count,
        "sample_count": len(intervals),
        "target_duration_s": target_duration_seconds,
        "run_duration_s": elapsed_seconds,
        "intervals": intervals,
        "passed": all(item["passed"] for item in intervals),
    }


def run_direct_production_writer_profile(
    benchmark_binary: Path,
    environment: dict[str, str],
    roster: CapacityRoster,
    requested_messages: int,
    sample_count: int,
    workers: int,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Measure the canonical Tokio admission writer without raw SQL or HTTP.

    The benchmark-owned daemon first creates the disposable roster and is then
    stopped.  The direct binary calls ``prepare_write_with_async_runtime`` so
    this lane includes the canonical write preparation, writer queue, batch
    transaction, commit, and post-commit response decision; it excludes only
    the public socket/codec measured by UDS and TCP.
    """
    direct_environment = dict(environment)
    direct_environment.update(
        {
            "ATM_CAPACITY_CORE_TEAM": roster.team,
            "ATM_CAPACITY_CORE_AGENT": roster.agent,
            "ATM_CAPACITY_CORE_RECIPIENT": roster.recipient,
        }
    )
    result = subprocess.run(
        [
            str(benchmark_binary), "--direct-core-write", str(requested_messages),
            "--workers", str(workers), "--intervals", str(sample_count),
            "--seconds", str(int(TARGET_PROFILE_DURATION_SECONDS)),
        ],
        cwd=ROOT,
        env=direct_environment,
        capture_output=True,
        text=True,
        check=False,
        timeout=TARGET_PROFILE_DURATION_SECONDS + 60.0,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no diagnostic output"
        raise SmokeError(f"direct production-writer benchmark failed: {detail}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SmokeError("direct production-writer benchmark returned malformed JSON") from error
    if not isinstance(payload, dict) or payload.get("kind") != "canonical_core_write":
        raise SmokeError("direct production-writer benchmark returned the wrong measurement kind")
    direct_intervals = payload.get("intervals")
    if not isinstance(direct_intervals, list) or len(direct_intervals) < sample_count:
        raise SmokeError("direct production-writer benchmark returned too few intervals")
    intervals: list[dict[str, Any]] = []
    for index, item in enumerate(direct_intervals, start=1):
        if not isinstance(item, dict):
            raise SmokeError("direct production-writer interval is malformed")
        accepted = int(item.get("accepted_count", -1))
        requested = int(item.get("requested_count", -1))
        elapsed = float(item.get("elapsed_seconds", 0.0))
        rate = float(item.get("admissions_per_second", 0.0))
        if requested != requested_messages or accepted < 0 or accepted > requested or elapsed <= 0 or rate < 0:
            raise SmokeError("direct production-writer interval has invalid counts or timing")
        intervals.append(
            {
                "interval": index,
                "accepted_count": accepted,
                "response_count": accepted,
                "elapsed_seconds": elapsed,
                "admissions_per_second": rate,
                "latency_ms": {"min": 0.0, "p50": 0.0, "p95": 0.0, "max": 0.0},
                "connections": 0,
                "connection_workers": 0,
                "request_frames_per_second": rate,
                "connections_per_second": 0.0,
                "requested_count": requested,
                "time_to_send_1k_s": elapsed * (1_000 / max(accepted, 1)),
                "application_wire_bytes": {"request": 0, "response": 0, "total": 0},
                "application_wire_bytes_per_second": 0.0,
                "error_free": accepted == requested,
                "bytes_per_second": 0.0,
                "first_failure": None if accepted == requested else "direct production writer accepted fewer messages than requested",
                "passed": accepted == requested,
            }
        )
    return (
        {
            "operation": "canonical_production_writer",
            "recipient": f"{roster.recipient}@{roster.team}",
            "requested_messages_per_sample": requested_messages,
            "minimum_sample_count": sample_count,
            "sample_count": len(intervals),
            "target_duration_s": TARGET_PROFILE_DURATION_SECONDS,
            "run_duration_s": sum(float(item["elapsed_seconds"]) for item in intervals),
            "intervals": intervals,
            "passed": all(item["passed"] for item in intervals),
        },
        payload,
    )


def run_cached_roster_heartbeat_probe(
    endpoint: LocalEndpoint,
    home: Path,
    frames_per_connection: int,
    workers: int,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> dict[str, Any]:
    """Warm and then measure the public no-SQLite heartbeat route."""
    warmup = submit_connection(endpoint, [
        HttpRequest(
            "/v1/atm/heartbeat",
            cached_roster_heartbeat_body(0, roster),
            200,
        ),
    ])
    if len(warmup) != 1 or warmup[0].status != 200 or warmup[0].failure is not None:
        detail = warmup[0].failure if warmup else "no response"
        raise SmokeError(f"cached-roster heartbeat warmup failed: {detail}")
    profile = run_profile(
        endpoint,
        home,
        frames_per_connection,
        ADMISSIONS_PER_INTERVAL,
        DIAGNOSTIC_SAMPLE_COUNT,
        workers,
        target_duration_seconds=DIAGNOSTIC_DURATION_SECONDS,
        operation="cached_roster_heartbeat",
        expected_status=200,
        minimum_admissions_per_second=0,
        roster=roster,
    )
    return {
        "route": "/v1/atm/heartbeat",
        "storage": "warmed LocalServiceRuntime roster snapshot; no SQLite reads after warmup",
        "warmup": {"status": warmup[0].status, "passed": True},
        "profile": profile,
    }


def evidence_filename(directory: Path, evidence: dict[str, Any]) -> Path:
    """Return the stable path used by both raw and public run artifacts."""
    directory.mkdir(parents=True, exist_ok=True)
    host_label = re.sub(r"[^A-Za-z0-9._-]+", "-", str(evidence["host_label"])).strip("-") or "host"
    # The report renderer derives its immutable artifact id from generated_at.
    # Derive this filename from the same value so aggregate JSON/XHTML links
    # cannot point to a different, merely wall-clock-adjacent filename.
    generated_at = str(evidence.get("generated_at") or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"))
    timestamp = generated_at.replace("-", "").replace(":", "").replace("T", "-").replace("Z", "")
    return directory / (
        f"{timestamp}-{host_label}-{evidence['transport']}-"
        f"f{evidence['frames_per_connection']}.json"
    )


def write_raw_evidence(directory: Path, evidence: dict[str, Any]) -> Path:
    """Write the full local-only trace; this directory is intentionally ignored."""
    path = evidence_filename(directory, evidence)
    path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def write_evidence(directory: Path, evidence: dict[str, Any]) -> Path:
    """Write a legacy v3 diagnostic artifact for read-only compatibility.

    The ordinary matrix path supplies ``V4EmissionContext`` to ``run_capacity``
    and therefore bypasses this compatibility writer entirely.
    """
    path = evidence_filename(directory, evidence)
    try:
        summary = compact_evidence(evidence).model_dump(mode="json")
    except BenchmarkSchemaError as error:
        raise SmokeError(f"could not summarize benchmark evidence: {error}") from error
    path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def selected_profiles(
    sparse_profiles: tuple[int, ...], sustained_profiles: tuple[int, ...],
) -> tuple[tuple[int, int], ...]:
    """Keep sparse samples ahead of each requested sustained transport profile."""
    profiles = [(frames, ADMISSIONS_PER_INTERVAL) for frames in sparse_profiles]
    profiles.extend(
        (frames, messages)
        for messages in sustained_profiles
        for frames in sparse_profiles
    )
    return tuple(profiles)


def run_capacity(
    atm_home: Path,
    evidence_directory: Path,
    transport: str,
    frames_per_connection: int,
    requested_messages: int = ADMISSIONS_PER_INTERVAL,
    sample_count: int = INTERVALS,
    workers: int = DEFAULT_WORKERS,
    raw_evidence_directory: Path = DEFAULT_RAW_EVIDENCE_DIR,
    peer_wire_security: str = "mutual-tls",
    managed_log_level: str | None = None,
    benchmark_target: str | None = None,
    managed_daemon: ManagedDaemonOptions | None = None,
    campaign_id: str | None = None,
    v4_emission: V4EmissionContext | None = None,
) -> CapacityRunResult:
    """Start one branch daemon, exercise public UDS API, retain evidence, then clean up."""
    if managed_daemon is not None:
        raise SmokeError(
            "managed-daemon benchmarking is retired because it would touch the primary "
            "OS-user-owned database; use a dedicated clean OS user instead"
        )
    benchmark_account = require_capacity_benchmark_account()
    transport = validate_transport(transport)
    if transport == "sqlite":
        if peer_wire_security is not None:
            raise SmokeError("sqlite capacity target must not select a peer-wire security mode")
        launch_peer_wire_security = "mutual-tls"
    else:
        if peer_wire_security is None:
            raise SmokeError("public capacity target requires an explicit peer-wire security mode")
        peer_wire_security = validate_peer_wire_security(peer_wire_security)
        launch_peer_wire_security = peer_wire_security
    if frames_per_connection not in SPARSE_FRAMES_PER_CONNECTION:
        raise SmokeError(f"frames per connection must be one of {SPARSE_FRAMES_PER_CONNECTION}")
    if requested_messages <= 0:
        raise SmokeError("requested messages must be positive")
    if workers <= 0:
        raise SmokeError("capacity worker limit must be positive")
    isolation_mode = "dedicated_benchmark_os_account"
    home = validate_capacity_home(atm_home)
    atm = release_binary("atm")
    daemon = release_binary("atm-daemon")
    direct_writer = canonical_writer_probe() if transport == "sqlite" else None
    roster = CapacityRoster.unique()
    env = runtime_environment(home, roster)
    direct_peer_port = allocate_direct_peer_port()
    env[CAPACITY_DIRECT_PEER_PORT_ENV] = str(direct_peer_port)
    target_command = (
        f"just benchmark --target {benchmark_target}"
        if benchmark_target is not None
        else f"just benchmark --transport {transport}"
    )
    process: subprocess.Popen[str] | None = None
    daemon_output: DaemonOutputCapture | None = None
    before: list[int] | None = None
    snapshot: VerifiedSnapshot | None = None
    mtls_identity: DisposableMtlsIdentity | None = None
    evidence: dict[str, Any] = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "campaign_id": campaign_id,
        "host_label": os.environ.get("ATM_CAPACITY_HOST_LABEL", "local"),
        "transport": transport,
        "peer_wire_security": peer_wire_security,
        "benchmark_target": benchmark_target,
        "hook_mode": "active",
        "frames_per_connection": frames_per_connection,
        "run_duration_s": None,
        "messages_per_connection": frames_per_connection,
        "requested_messages_per_sample": requested_messages,
        "minimum_sample_count": sample_count,
        "sample_count": None,
        "target_duration_s": TARGET_PROFILE_DURATION_SECONDS,
        "worker_limit": workers,
        "direct_peer_port": direct_peer_port,
        "source_revision": source_revision(),
        "daemon_version": None,
        "host_os": platform.system().lower(),
        "host_arch": platform.machine().lower(),
        "command": target_command,
        "release": {"atm": str(atm), "atm_daemon": str(daemon)},
        "execution_daemon": (
            "direct_production_writer" if transport == "sqlite" else "shipped_atm_daemon"
        ),
        "atm_home": str(home),
        "host_state_isolation": isolation_mode,
        "managed_log_level": managed_log_level,
        "benchmark_account": {
            "account_identity": (
                f"sha256:{hashlib.sha256(str(benchmark_account.account_id).encode()).hexdigest()[:16]}"
            ),
        },
        "lifecycle": {},
        "runs": [],
        "stages": {
            "runtime_view_validation": "daemon-owned; no peer/store/network work is requested by this client before response",
            "sqlite_transaction": "measured by each public admission response latency",
            "post_commit_received_hook": "active in the shipped daemon; any hook failure is returned as a successful write warning",
            "response_write": "included in each public admission response latency",
        },
        "operational_checks": {},
    }
    started_at = time.monotonic()

    def stop_owned_daemon(*, output_key: str | None = None) -> None:
        """Stop only the runner-owned daemon and retain its bounded diagnostics."""
        nonlocal process, daemon_output
        if process is None:
            return
        reap_owned_daemon(process)
        if daemon_output is not None:
            daemon_output.join()
            if output_key is not None:
                evidence[output_key] = daemon_output.evidence()
        process = None
        daemon_output = None

    def start_and_doctor(mode: str = launch_peer_wire_security) -> None:
        """Start the exact released daemon and require its public ready response."""
        nonlocal process, daemon_output
        process, daemon_output = start_capacity_daemon(daemon, home, env, mode)
        doctor = command_result(
            [str(atm), "doctor", "--json"],
            timeout=10.0,
            env=host_runtime_client_environment(env),
        )
        evidence["doctor"] = benchmark_doctor_payload(doctor)
        evidence["doctor_status"] = "passed"
        evidence["daemon_pid"] = process.pid

    try:
        run_lifecycle_phase(
            evidence,
            "snapshot",
            lambda: require_clean_host_daemon_state(smoke_label="admission-capacity smoke"),
        )
        before = count_atm_daemon_processes()
        home.mkdir(parents=True, exist_ok=False)
        if launch_peer_wire_security == "mutual-tls":
            # Every mTLS daemon launch validates its configured identity,
            # including the secure-default daemon used by the sqlite and UDS
            # targets.  Persist a disposable identity through the ordinary
            # plaintext control plane before any measured mTLS launch.
            run_lifecycle_phase(evidence, "snapshot", lambda: start_and_doctor("plaintext-test"))
            mtls_identity = run_lifecycle_phase(
                evidence,
                "snapshot",
                lambda: provision_disposable_mtls_identity(
                    atm, env, home, roster, direct_peer_port,
                ),
            )
            evidence["disposable_mtls"] = {
                "authority": mtls_identity.host,
                "fingerprint_sha256": mtls_identity.fingerprint,
                "direct_peer_port": direct_peer_port,
            }
            run_lifecycle_phase(
                evidence,
                "stop",
                lambda: stop_owned_daemon(output_key="mtls_setup_daemon_output"),
            )
        # The pre-roster daemon is deliberately short-lived: it initializes
        # the account database, then is quiesced before the public snapshot
        # owner copies the clean baseline.
        run_lifecycle_phase(evidence, "snapshot", start_and_doctor)
        run_lifecycle_phase(
            evidence, "stop", lambda: stop_owned_daemon(output_key="pre_snapshot_daemon_output"),
        )
        snapshot = run_lifecycle_phase(evidence, "snapshot", create_verified_snapshot)
        evidence["clean_baseline_snapshot"] = snapshot_evidence(snapshot)
        evidence["clean_baseline_snapshot"]["sidecars_absent"] = True

        # Roster creation belongs strictly after clean-baseline publication.
        run_lifecycle_phase(evidence, "profile", start_and_doctor)
        run_lifecycle_phase(
            evidence, "profile", lambda: prepare_capacity_roster(atm, env, home, roster),
        )
        if transport == "sqlite":
            if direct_writer is None:
                raise SmokeError("direct production-writer binary is unavailable")
            run_lifecycle_phase(evidence, "stop", stop_owned_daemon)
            profile, direct_measurement = run_lifecycle_phase(
                evidence,
                "profile",
                lambda: run_direct_production_writer_profile(
                    direct_writer, env, roster, requested_messages, sample_count, workers,
                ),
            )
            evidence["direct_sqlite_message_write"] = {
                field: direct_measurement[field]
                for field in (
                    "kind", "requested_count", "accepted_count", "worker_count",
                    "elapsed_seconds", "admissions_per_second",
                )
            }
        else:
            endpoint = (
                direct_peer_endpoint(
                    direct_peer_port,
                    mtls_identity if peer_wire_security == "mutual-tls" else None,
                )
                if transport == "tcp"
                else local_endpoint(transport)
            )
            evidence["endpoint"] = {
                "transport": endpoint.kind,
                "address": endpoint.address,
                "direct_peer": endpoint.direct_peer,
                "tls_server_name": endpoint.tls_server_name,
            }
            if endpoint.kind == "uds" and not Path(str(endpoint.address)).exists():
                raise SmokeError(f"daemon did not publish public local socket {endpoint.address}")
            if not endpoint.direct_peer:
                evidence["operational_checks"]["cached_roster_heartbeat"] = run_lifecycle_phase(
                    evidence,
                    "profile",
                    lambda: run_cached_roster_heartbeat_probe(
                        endpoint, home, frames_per_connection, workers, roster,
                    ),
                )
            profile = run_lifecycle_phase(
                evidence,
                "profile",
                lambda: run_profile(
                    endpoint, home, frames_per_connection, requested_messages,
                    sample_count, workers, roster=roster,
                ),
            )
        evidence["runs"] = [profile]
        evidence["sample_count"] = profile["sample_count"]
        evidence["target_duration_s"] = profile["target_duration_s"]
        evidence["run_duration_s"] = profile["run_duration_s"]
        evidence["daemon_version"] = release_version(atm)

        # A 201/direct-writer success is not enough for a benchmark result:
        # reopen the isolated account store with a fresh shipped daemon, then
        # count this target's unique recipient mailbox.  This happens before
        # the clean snapshot is restored and is intentionally outside every
        # timed interval.
        run_lifecycle_phase(evidence, "stop", stop_owned_daemon)
        run_lifecycle_phase(evidence, "restart", start_and_doctor)
        evidence["doctor_after_restart"] = {"status": evidence["doctor_status"]}
        expected_accepted_count = sum(
            int(interval["accepted_count"]) for interval in profile["intervals"]
        )
        evidence["durability_after_restart"] = run_lifecycle_phase(
            evidence,
            "durability",
            lambda: verify_durability_after_restart(
                benchmark_account,
                roster,
                expected_accepted_count,
            ),
        )

        # v4 applies reviewed per-host/target floors only after this runner
        # has completed its lifecycle and emitted its factual measurements.
        evidence["passed"] = (
            all(bool(item.get("passed")) for item in profile["intervals"])
            and bool(evidence["durability_after_restart"]["passed"])
        )
    except (OSError, RuntimeError, ValueError, SmokeError) as error:
        evidence["passed"] = False
        evidence["failure"] = str(error)
    finally:
        # A setup or baseline-validation failure can occur before a profile
        # exists.  Public failed-run evidence still has to satisfy the
        # summary schema, so retain the elapsed wall time rather than leaving
        # its required duration field null.
        if evidence["run_duration_s"] is None:
            evidence["run_duration_s"] = time.monotonic() - started_at
        if process is not None:
            try:
                run_lifecycle_phase(evidence, "stop", stop_owned_daemon)
            except SmokeError as error:
                evidence["passed"] = False
                evidence["failure"] = str(error)
        if snapshot is not None and process is None:
            try:
                restored = run_lifecycle_phase(
                    evidence, "restore", lambda: restore_verified_snapshot(snapshot.snapshot_id),
                )
                evidence["restored_clean_baseline"] = snapshot_evidence(restored)

                def verify_clean_baseline() -> None:
                    # Keep the daemon stopped: a post-restore start can recreate
                    # SQLite sidecars and invalidate the exact clean baseline.
                    verified = verify_active_snapshot(snapshot.snapshot_id)
                    evidence["post_restore_snapshot"] = snapshot_evidence(verified)
                    evidence["restored_live_database"] = {
                        **snapshot_evidence(verified),
                        "sidecars_absent": True,
                    }

                run_lifecycle_phase(evidence, "post_restore_verify", verify_clean_baseline)
                run_lifecycle_phase(evidence, "cleanup", stop_owned_daemon)
            except SmokeError as error:
                evidence["passed"] = False
                evidence["failure"] = str(error)
        elif snapshot is not None:
            evidence["passed"] = False
            evidence["failure"] = (
                "benchmark stop phase failed; recovery: keep the benchmark daemon stopped and do not restore "
                "while SQLite sidecars may be active"
            )
        if daemon_output is not None:
            daemon_output.join()
            evidence["daemon_output"] = daemon_output.evidence()
        if before is not None:
            try:
                assert_no_process_leak(
                    before, count_atm_daemon_processes(), smoke_label="admission-capacity smoke",
                )
            except RuntimeError as error:
                evidence["passed"] = False
                evidence["cleanup_failure"] = str(error)
        def remove_temporary_runtime() -> None:
            if home.exists():
                for child in sorted(home.rglob("*"), reverse=True):
                    if child.is_file() or child.is_symlink():
                        child.unlink()
                    elif child.is_dir():
                        child.rmdir()
                home.rmdir()

        try:
            run_lifecycle_phase(evidence, "cleanup", remove_temporary_runtime)
        except SmokeError as error:
            evidence["passed"] = False
            evidence["failure"] = str(error)
        raw_evidence_path = write_raw_evidence(raw_evidence_directory, evidence)
        published_result: BenchmarkRunResult | None = None
        if v4_emission is None:
            evidence_path = write_evidence(evidence_directory, evidence)
        else:
            evidence_path, published_result = write_v4_evidence(
                evidence_directory, evidence, v4_emission,
            )
        print(f"local benchmark trace: {raw_evidence_path}")
    return CapacityRunResult(
        0 if evidence.get("passed") else 1,
        evidence_path,
        raw_evidence_path,
        published_result,
    )


def run_required_f8_suite(args: argparse.Namespace) -> int:
    """Run the ordinary command's unskippable sqlite/UDS/TCP/TLS matrix.

    AO2.7 keeps target selection out of the ordinary ``just benchmark`` path:
    its one result is a complete f8-v1 suite.  The legacy selectors below are
    retained only for focused diagnostic tests and cannot represent the
    default release benchmark command.
    """
    if any((args.target, args.transport, args.frames_per_connection, args.sustained)):
        raise SmokeError("the required f8 suite does not permit target, transport, or profile selection")
    source = source_revision()
    host_label = os.environ.get("ATM_CAPACITY_HOST_LABEL", "local")
    started_at = datetime.now(timezone.utc)
    campaign_identifier = derive_campaign_id(started_at=started_at, host_label=host_label)
    baselines = load_baselines()
    os_name = benchmark_os()
    target_matrix = tuple(
        (target, configuration)
        for target, configuration in BENCHMARK_TARGETS.items()
        if target in required_targets(os_name)
    )
    # Fail before exercising any target when the reviewed policy has no floor;
    # this makes an accidental local/default run explicit rather than publish a
    # result with a silent or caller-selected baseline.
    try:
        target_baselines = {
            target: baselines.entry_for(host_label, target)
            for target, _configuration in target_matrix
        }
    except BenchmarkSchemaError as error:
        raise SmokeError(
            "required benchmark campaign has no reviewed baseline: "
            f"{error}"
        ) from error
    hashes = binary_hashes()
    print(f"benchmark campaign: {campaign_identifier}")
    codes: list[int] = []
    published_results: list[BenchmarkRunResult] = []
    for position, (target, (transport, peer_wire_security)) in enumerate(target_matrix, start=1):
        emission = V4EmissionContext(
            target=target,
            campaign_id=campaign_identifier,
            os_name=os_name,
            baseline=target_baselines[target],
            binary_hashes=hashes,
        )
        if args.atm_home is None:
            with tempfile.TemporaryDirectory(prefix="atm-capacity-parent-") as temporary:
                run = run_capacity(
                    Path(temporary) / f"{CAPACITY_ROOT_PREFIX}{position}",
                    args.evidence_dir,
                    transport,
                    8,
                    ADMISSIONS_PER_INTERVAL,
                    INTERVALS,
                    args.workers,
                    raw_evidence_directory=args.raw_evidence_dir,
                    peer_wire_security=peer_wire_security,
                    benchmark_target=target,
                    campaign_id=campaign_identifier,
                    v4_emission=emission,
                )
        else:
            run = run_capacity(
                args.atm_home / f"{CAPACITY_ROOT_PREFIX}{position}",
                args.evidence_dir,
                transport,
                8,
                ADMISSIONS_PER_INTERVAL,
                INTERVALS,
                args.workers,
                raw_evidence_directory=args.raw_evidence_dir,
                peer_wire_security=peer_wire_security,
                benchmark_target=target,
                campaign_id=campaign_identifier,
                v4_emission=emission,
            )
        codes.append(run.code)
        if run.result is None:
            codes.append(1)
            print(f"FAIL required f8 target {target}: direct v4 publication failed")
            continue
        result = run.result
        published_results.append(result)
        metrics = result.metrics
        print(
            f"{result.status} required f8 target {target}: "
            f"p50={'n/a' if metrics is None else f'{metrics.admissions_per_second.p50:.2f}'} msg/s "
            f"accepted={result.messages_admitted}/{result.messages_requested} "
            f"evidence={run.compact_evidence_path} raw={run.raw_evidence_path}"
        )
    campaign_status = (
        "INCOMPLETE" if len(published_results) != len(target_matrix)
        else "FAIL" if any(result.status == "FAIL" for result in published_results)
        else "INCOMPLETE" if any(result.status == "INCOMPLETE" for result in published_results)
        else "PASS"
    )
    campaign = BenchmarkCampaign(
        campaign_id=campaign_identifier,
        host_label=host_label,
        os=os_name,
        phase="ao2",
        started_at=started_at,
        completed_at=datetime.now(timezone.utc),
        source_revision=source,
        results=tuple(published_results),
        status=campaign_status,
    )
    atomic_json(
        args.evidence_dir / f"{campaign_identifier}.campaign.json",
        campaign.model_dump(mode="json"),
    )
    return 0 if all(code == 0 for code in codes) and campaign.status == "PASS" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bootstrap-benchmark-account",
        action="store_true",
        help="create the account-local benchmark manifest; refuses an account with existing ATM state",
    )
    parser.add_argument("--atm-home", type=Path)
    parser.add_argument(
        "--diagnostic-only",
        action="store_true",
        help="permit a selected historical profile; its output is not a complete benchmark suite",
    )
    parser.add_argument(
        "--evidence-dir", type=Path,
        default=DEFAULT_EVIDENCE_DIR,
        help="committed compact benchmark summary directory (default: site/reports/send-message-benchmark)",
    )
    parser.add_argument(
        "--raw-evidence-dir", type=Path,
        default=DEFAULT_RAW_EVIDENCE_DIR,
        help="ignored local interval-trace directory (default: artifacts/benchmark/send-message-benchmark)",
    )
    parser.add_argument(
        "--target",
        choices=tuple(BENCHMARK_TARGETS),
        help=(
            "one focused benchmark target; without a target the ordinary command runs "
            "the required sqlite, UDS, plaintext TCP, and mTLS TCP f8 matrix"
        ),
    )
    parser.add_argument("--transport")
    parser.add_argument("--workers", type=int, default=DEFAULT_WORKERS)
    parser.add_argument(
        "--frames-per-connection",
        type=int,
        action="append",
        choices=SPARSE_FRAMES_PER_CONNECTION,
        help="one sparse profile; repeat to select a subset (default: all sparse profiles)",
    )
    parser.add_argument(
        "--sustained",
        type=int,
        action="append",
        choices=SUSTAINED_MESSAGE_COUNTS,
        help="add one 10K or 100K sustained profile after the sparse baseline",
    )
    args = parser.parse_args()
    if args.bootstrap_benchmark_account:
        try:
            account = bootstrap_benchmark_account()
        except BenchmarkAccountError as error:
            raise SmokeError(f"benchmark-account bootstrap failed: {error}") from error
        print(f"benchmark-account manifest created: {account.manifest_path}")
        return 0
    selected_profile = any(
        (args.target, args.transport, args.frames_per_connection, args.sustained)
    )
    if not selected_profile:
        if args.diagnostic_only:
            raise SmokeError("diagnostic-only requires an explicit selected profile")
        return run_required_f8_suite(args)
    if not args.diagnostic_only:
        raise SmokeError("selected benchmark profiles require --diagnostic-only and cannot be suite evidence")
    transport, peer_wire_security, benchmark_target = resolve_benchmark_target(
        args.target, args.transport,
    )
    sparse_profiles = tuple(args.frames_per_connection or SPARSE_FRAMES_PER_CONNECTION)
    sustained_profiles = tuple(args.sustained or ())
    profiles = selected_profiles(sparse_profiles, sustained_profiles)
    codes: list[int] = []
    for position, (frames_per_connection, requested_messages) in enumerate(profiles, start=1):
        if args.atm_home is None:
            with tempfile.TemporaryDirectory(prefix="atm-capacity-parent-") as temp:
                home = Path(temp) / f"{CAPACITY_ROOT_PREFIX}{position}"
                code, evidence = run_capacity(
                    home, args.evidence_dir, transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    raw_evidence_directory=args.raw_evidence_dir,
                    peer_wire_security=peer_wire_security,
                    benchmark_target=benchmark_target,
                )
        else:
            home = args.atm_home / f"{CAPACITY_ROOT_PREFIX}{position}"
            code, evidence = run_capacity(
                    home, args.evidence_dir, transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    raw_evidence_directory=args.raw_evidence_dir,
                    peer_wire_security=peer_wire_security,
                    benchmark_target=benchmark_target,
                )
        codes.append(code)
        print(f"{'PASS' if code == 0 else 'FAIL'} admission-capacity evidence: {evidence}")
    return 0 if all(code == 0 for code in codes) else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"capacity smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

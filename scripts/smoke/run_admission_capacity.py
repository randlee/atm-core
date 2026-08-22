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
import socket
import sqlite3
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

from scripts.smoke.benchmark_schema import BenchmarkSchemaError, compact_evidence, distribution
from scripts.smoke.benchmark_policy import (
    baseline_reference,
    evaluate_profile_thresholds,
    load_baseline_median,
    profile_median_admissions_per_second,
    validated_profile_median,
)
from scripts.smoke.benchmark_account import (
    BenchmarkAccount,
    BenchmarkAccountError,
    bootstrap_benchmark_account,
    require_benchmark_account,
)
from scripts.smoke.benchmark_snapshot import (
    BenchmarkSnapshotError,
    VerifiedSnapshot,
    create_verified_snapshot,
    restore_verified_snapshot,
    verify_completed_snapshot,
)

if os.name != "nt":
    import pwd

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
TCP_COMPARISON_FRAMES = (1, 2, 4, 8, 16, 64)
SUSTAINED_MESSAGE_COUNTS = (10_000, 100_000)
DAEMON_OUTPUT_TAIL_LINES = 200
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")
PEER_WIRE_SECURITY_MODES = ("mutual-tls", "plaintext-test")
BENCHMARK_TARGETS = {
    "tcp": ("tcp", "plaintext-test"),
    "tcp-tls": ("tcp", "mutual-tls"),
}
MISSING_PLAINTEXT_BASELINE = "missing_compatible_plaintext_baseline"
DAEMON_SWITCH = ROOT / ".claude" / "skills" / "daemon-switch" / "scripts" / "daemon-switch.py"
# The daemon-switch control plane can legitimately wait through its documented
# launchctl unload/owner-repair windows (up to 20s + two 20x2s polls).  Its
# outer timeout must cover that bounded recovery path; otherwise the runner
# reports a false benchmark failure while the switch is still repairing the
# selected singleton.
MANAGED_DAEMON_TIMEOUT_SECONDS = 120.0
DIAGNOSTIC_SAMPLE_COUNT = 3
DIAGNOSTIC_DURATION_SECONDS = 3.0


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
    """One authenticated public local daemon endpoint."""

    kind: str
    address: str | tuple[str, int]
    capability: str | None = None


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
    "stop": "keep the benchmark daemon stopped; do not restore while SQLite sidecars may be active",
    "restore": "keep the benchmark daemon stopped and inspect retained restore staging material",
    "post_restore_verify": "keep the benchmark daemon stopped and inspect the restored benchmark account",
    "cleanup": "remove only the per-run temporary ATM_HOME after inspecting the retained evidence",
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
    except (BenchmarkSnapshotError, OSError, RuntimeError, ValueError, SmokeError, subprocess.TimeoutExpired) as error:
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
    process = subprocess.Popen(
        [
            str(daemon),
            "--peer-wire-security", peer_wire_security,
        ],
        cwd=home,
        env=env,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    output = DaemonOutputCapture.start(process)
    try:
        await_daemon_ready(process, output)
    except (OSError, SmokeError):
        reap_owned_daemon(process)
        output.join()
        raise
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
    return json.dumps(payload, separators=(",", ":")).encode("utf-8")


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
    if transport not in {"uds", "tcp"}:
        raise SmokeError("capacity transport must be `uds` or `tcp`")
    if os.name == "nt" and transport != "tcp":
        raise SmokeError("Windows capacity benchmarking supports only `tcp`")
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
) -> tuple[str, str, str | None]:
    """Resolve public targets without inventing a benchmark-only transport.

    `tcp` deliberately selects the existing direct plaintext pipeline and
    `tcp-tls` selects the same daemon with its ordinary mutual-TLS launch
    argument.  Legacy `--transport` remains available for historical UDS
    evidence, but it defaults to the secure daemon mode.
    """
    if target is not None:
        selected_transport, peer_wire_security = BENCHMARK_TARGETS[target]
        if transport is not None and transport != selected_transport:
            raise SmokeError(
                f"benchmark target {target!r} requires transport {selected_transport!r}"
            )
        return selected_transport, peer_wire_security, target
    return validate_transport(transport or ("uds" if os.name != "nt" else "tcp")), "mutual-tls", None


def local_endpoint(transport: str) -> LocalEndpoint:
    """Resolve the documented UDS/TCP public API without a dispatcher seam."""
    runtime = os_account_home() / ".atm" / "daemon"
    if transport == "uds":
        return LocalEndpoint("uds", str(runtime / "atm-daemon.sock"))
    try:
        record = json.loads((runtime / "local-http.json").read_text(encoding="utf-8"))
        address = record["ipv4_loopback"]
        capability = record["capability_base64url"]
        if not isinstance(address, str) or not isinstance(capability, str):
            raise ValueError("missing loopback endpoint or capability")
        host, port = address.rsplit(":", 1)
        return LocalEndpoint("tcp", (host, int(port)), capability)
    except (OSError, ValueError, json.JSONDecodeError, KeyError) as error:
        raise SmokeError(f"could not read daemon local HTTP endpoint record: {error}") from error


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
    """Submit consecutive real requests over one public local connection."""
    started = time.perf_counter()
    capability = (
        f"X-ATM-Local-Capability: {endpoint.capability}\r\n".encode("ascii")
        if endpoint.capability is not None
        else b""
    )
    results: list[AdmissionResult] = []
    try:
        family = socket.AF_UNIX if endpoint.kind == "uds" else socket.AF_INET
        with socket.socket(family, socket.SOCK_STREAM) as stream:
            stream.settimeout(3.5)
            if endpoint.kind == "tcp":
                stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            stream.connect(endpoint.address)
            frames = []
            for index, request in enumerate(requests):
                connection = "close" if index + 1 == len(requests) else "keep-alive"
                frames.append(
                    f"POST {request.path} HTTP/1.1\r\n".encode("ascii")
                    + b"Content-Type: application/json\r\n"
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
        return results
    except (OSError, SmokeError) as error:
        return results + [AdmissionResult(
            status=0, elapsed_ms=(time.perf_counter() - started) * 1_000,
            failure=str(error),
        )]


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
    with ThreadPoolExecutor(max_workers=workers, thread_name_prefix="atm-capacity") as executor:
        connections = (requested_messages + frames_per_connection - 1) // frames_per_connection
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
                    http_request_body(home, sequence + offset, roster),
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
    """Write the Pydantic-validated compact public benchmark summary."""
    path = evidence_filename(directory, evidence)
    try:
        summary = compact_evidence(evidence).model_dump(mode="json")
    except BenchmarkSchemaError as error:
        raise SmokeError(f"could not summarize benchmark evidence: {error}") from error
    path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def verify_durable_admissions(
    db_path: Path,
    expected_count: int,
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> dict[str, int | bool | str]:
    """Count every benchmark admission in the isolated store after restart.

    Admission itself always traverses the public authenticated HTTP boundary.
    The post-restart proof intentionally reads the disposable SQLite store
    directly: general mailbox listing builds a logical projection of every row
    and is not a bounded durability-count API at large volume.
    """
    try:
        uri = f"{db_path.resolve().as_uri()}?mode=ro"
        with closing(sqlite3.connect(uri, uri=True)) as connection:
            row = connection.execute(
                "SELECT COUNT(*) FROM mail_messages WHERE team = ?1 AND agent = ?2;",
                (roster.team, roster.recipient),
            ).fetchone()
        observed_count = int(row[0]) if row is not None else 0
    except (OSError, sqlite3.Error, TypeError, ValueError) as error:
        raise SmokeError(f"capacity durability count failed: {error}") from error
    if observed_count != expected_count:
        raise SmokeError(
            "capacity durability mismatch after daemon restart: "
            f"expected {expected_count}, observed {observed_count}"
        )
    return {
        "method": "isolated_sqlite_exact_count_after_restart",
        "expected_accepted_count": expected_count,
        "observed_mailbox_count": observed_count,
        "passed": True,
    }


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


def matching_profile_reference(
    directory: Path,
    host_label: str,
    transport: str,
    frames_per_connection: int,
    revision: str,
    peer_wire_security: str = "plaintext-test",
) -> tuple[float, str]:
    """Use only one complete accepted same-mode revision for comparison."""
    by_revision: dict[str, dict[int, tuple[str, float]]] = {}
    for path in directory.glob("*.json"):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            candidate_revision = payload.get("source_revision")
            candidate_frame = payload.get("frames_per_connection")
            if (
                payload.get("host_label") == host_label
                and payload.get("transport") == transport
                and payload.get("peer_wire_security") == peer_wire_security
                and payload.get("execution_daemon") == "shipped_atm_daemon"
                and candidate_frame in TCP_COMPARISON_FRAMES
                and isinstance(candidate_revision, str)
                and GIT_REVISION.fullmatch(candidate_revision)
                and is_ancestor_revision(candidate_revision, revision)
            ):
                by_revision.setdefault(candidate_revision, {})[candidate_frame] = (
                    str(payload.get("generated_at", "")),
                    validated_profile_median(payload, "comparison evidence"),
                )
        except (OSError, TypeError, ValueError, json.JSONDecodeError, SmokeError):
            continue
    complete = {
        candidate_revision: profiles
        for candidate_revision, profiles in by_revision.items()
        if all(frame in profiles for frame in TCP_COMPARISON_FRAMES)
    }
    if not complete:
        raise SmokeError(
            "missing a complete passed comparison set for host "
            f"{host_label}, mode {peer_wire_security}, at or before source revision {revision}"
        )
    selected_revision, profiles = max(
        complete.items(),
        key=lambda item: max(generated for generated, _median in item[1].values()),
    )
    _generated_at, median = profiles[frames_per_connection]
    return median, selected_revision


def baseline_comparison_reference(
    path: Path | None,
    peer_wire_security: str | None = None,
) -> tuple[float | None, str | None]:
    """Return a durable baseline's median and source revision for public evidence."""
    reference = baseline_reference(path)
    if reference is None:
        return None, None
    if (
        peer_wire_security is not None
        and (
            reference.get("peer_wire_security") != peer_wire_security
            or reference.get("execution_daemon") != "shipped_atm_daemon"
        )
    ):
        raise SmokeError(
            "capacity peer-wire baseline must record the matching explicit mode "
            "and execution_daemon=shipped_atm_daemon"
        )
    revision = reference.get("source_revision")
    if not isinstance(revision, str) or not GIT_REVISION.fullmatch(revision):
        raise SmokeError("capacity baseline must record a full source_revision for comparison")
    return float(reference["median_admissions_per_second"]), revision


def run_capacity(
    atm_home: Path,
    evidence_directory: Path,
    transport: str,
    frames_per_connection: int,
    requested_messages: int = ADMISSIONS_PER_INTERVAL,
    sample_count: int = INTERVALS,
    workers: int = DEFAULT_WORKERS,
    baseline_path: Path | None = None,
    comparison_median: float | None = None,
    comparison_source_revision: str | None = None,
    comparison_host_label: str | None = None,
    comparison_ratio: float = 1.0,
    comparison_strict: bool = False,
    comparison_required: bool = True,
    raw_evidence_directory: Path = DEFAULT_RAW_EVIDENCE_DIR,
    peer_wire_security: str = "mutual-tls",
    managed_log_level: str | None = None,
    benchmark_target: str | None = None,
    managed_daemon: ManagedDaemonOptions | None = None,
    preflight_failure_code: str | None = None,
    preflight_failure: str | None = None,
) -> tuple[int, Path]:
    """Start one branch daemon, exercise public UDS API, retain evidence, then clean up."""
    if managed_daemon is not None:
        raise SmokeError(
            "managed-daemon benchmarking is retired because it would touch the primary "
            "OS-user-owned database; use a dedicated clean OS user instead"
        )
    benchmark_account = require_capacity_benchmark_account()
    transport = validate_transport(transport)
    peer_wire_security = validate_peer_wire_security(peer_wire_security)
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
    roster = CapacityRoster.unique()
    env = runtime_environment(home, roster)
    target_command = (
        f"just benchmark --target {benchmark_target}"
        if benchmark_target is not None
        else f"just benchmark --transport {transport}"
    )
    process: subprocess.Popen[str] | None = None
    daemon_output: DaemonOutputCapture | None = None
    before: list[int] | None = None
    snapshot: VerifiedSnapshot | None = None
    evidence: dict[str, Any] = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
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
        "source_revision": source_revision(),
        "daemon_version": None,
        "host_os": platform.system().lower(),
        "host_arch": platform.machine().lower(),
        "command": target_command,
        "release": {"atm": str(atm), "atm_daemon": str(daemon)},
        "execution_daemon": "shipped_atm_daemon",
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
        "thresholds": None,
        "comparison_source_revision": comparison_source_revision,
        "comparison_host_label": comparison_host_label,
        "benchmark_evidence_failure_code": preflight_failure_code,
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

    def start_and_doctor() -> None:
        """Start the exact released daemon and require its public ready response."""
        nonlocal process, daemon_output
        process, daemon_output = start_capacity_daemon(daemon, home, env, peer_wire_security)
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
        endpoint = local_endpoint(transport)
        evidence["endpoint"] = {
            "transport": endpoint.kind,
            "address": endpoint.address,
        }
        if endpoint.kind == "uds" and not Path(str(endpoint.address)).exists():
            raise SmokeError(f"daemon did not publish public local socket {endpoint.address}")
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

        # Preserve the completed public profile before validating its
        # comparison reference. A stale or failed baseline must make the run
        # fail closed, but it must not erase the measurements that explain
        # that failure from the compact evidence.
        baseline_median = load_baseline_median(
            baseline_path, transport, frames_per_connection, peer_wire_security,
        )
        evidence["baseline"] = baseline_reference(baseline_path)
        evidence["thresholds"] = evaluate_profile_thresholds(
            profile, baseline_median, comparison_median, comparison_ratio,
            comparison_strict, comparison_required,
        )
        evidence["passed"] = evidence["thresholds"]["passed"]
        # A missing plaintext comparison baseline blocks acceptance, but it
        # must not prevent collection of the bounded profile that explains the
        # gap. Preserve the measured run and durability proof, then fail closed.
        if preflight_failure is not None:
            evidence["passed"] = False
            evidence["failure"] = preflight_failure
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
                    start_and_doctor()
                    verified = verify_completed_snapshot(snapshot.snapshot_id)
                    evidence["post_restore_snapshot"] = snapshot_evidence(verified)
                    evidence["doctor_after_restore"] = {"status": "passed"}

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
        evidence_path = write_evidence(evidence_directory, evidence)
        print(f"local benchmark trace: {raw_evidence_path}")
    return (0 if evidence.get("passed") else 1), evidence_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bootstrap-benchmark-account",
        action="store_true",
        help="create the account-local benchmark manifest; refuses an account with existing ATM state",
    )
    parser.add_argument("--atm-home", type=Path)
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
            "public peer-wire benchmark target: `tcp` selects plaintext-test; "
            "`tcp-tls` selects mutual TLS"
        ),
    )
    parser.add_argument("--transport")
    parser.add_argument("--workers", type=int, default=DEFAULT_WORKERS)
    parser.add_argument(
        "--baseline",
        type=Path,
        help="compatible prior evidence artifact whose median this profile must meet",
    )
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
    transport, peer_wire_security, benchmark_target = resolve_benchmark_target(
        args.target, args.transport,
    )
    sparse_profiles = tuple(args.frames_per_connection or SPARSE_FRAMES_PER_CONNECTION)
    sustained_profiles = tuple(args.sustained or ())
    profiles = selected_profiles(sparse_profiles, sustained_profiles)
    codes: list[int] = []
    current_revision = source_revision()
    host_label = os.environ.get("ATM_CAPACITY_HOST_LABEL", "local")
    comparison_host_label = os.environ.get(
        "ATM_CAPACITY_COMPARISON_HOST_LABEL",
        "mac-arm64-01" if os.name == "nt" else host_label,
    )
    uds_one_frame_median: float | None = None
    for position, (frames_per_connection, requested_messages) in enumerate(profiles, start=1):
        comparison_median: float | None = None
        comparison_source_revision: str | None = None
        comparison_ratio = 1.0
        comparison_strict = False
        comparison_required = True
        preflight_failure_code: str | None = None
        preflight_failure: str | None = None
        profile_baseline = None
        if transport == "uds":
            if frames_per_connection == 1:
                profile_baseline = args.baseline
                comparison_median, comparison_source_revision = baseline_comparison_reference(
                    profile_baseline,
                )
                if comparison_source_revision is not None:
                    comparison_host_label = host_label
            else:
                if uds_one_frame_median is None:
                    raise SmokeError("UDS multi-frame profile requires the current UDS one-frame reference")
                comparison_median = uds_one_frame_median
                comparison_strict = True
        else:
            # Connection setup dominates one/two-frame TCP.  Keep an explicit
            # short-frame floor instead of hiding it, while retaining the
            # stricter batching-parity floor where frames amortize setup.
            comparison_ratio = 0.9 if frames_per_connection >= 8 else 0.75
            comparison_required = os.name != "nt"
            try:
                comparison_median, comparison_source_revision = matching_profile_reference(
                    args.evidence_dir,
                    comparison_host_label,
                    transport,
                    frames_per_connection,
                    current_revision,
                    peer_wire_security,
                )
            except SmokeError:
                # The first accepted mutual-TLS campaign establishes its own
                # same-mode baseline. Plaintext cannot use that exception:
                # its pre-AO direct-TCP baseline is the regression gate.
                if peer_wire_security == "mutual-tls":
                    comparison_required = False
                else:
                    preflight_failure_code = MISSING_PLAINTEXT_BASELINE
                    preflight_failure = (
                        "missing a complete passed same-host plaintext baseline; "
                        "this run is retained as bounded benchmark evidence rather than "
                        "being discarded before publication"
                    )
        if args.atm_home is None:
            with tempfile.TemporaryDirectory(prefix="atm-capacity-parent-") as temp:
                home = Path(temp) / f"{CAPACITY_ROOT_PREFIX}{position}"
                code, evidence = run_capacity(
                    home, args.evidence_dir, transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    baseline_path=profile_baseline,
                    comparison_median=comparison_median,
                    comparison_source_revision=comparison_source_revision,
                    comparison_host_label=comparison_host_label,
                    comparison_ratio=comparison_ratio,
                    comparison_strict=comparison_strict,
                    comparison_required=comparison_required,
                    raw_evidence_directory=args.raw_evidence_dir,
                    peer_wire_security=peer_wire_security,
                    benchmark_target=benchmark_target,
                    preflight_failure_code=preflight_failure_code,
                    preflight_failure=preflight_failure,
                )
        else:
            home = args.atm_home / f"{CAPACITY_ROOT_PREFIX}{position}"
            code, evidence = run_capacity(
                    home, args.evidence_dir, transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    baseline_path=profile_baseline,
                    comparison_median=comparison_median,
                    comparison_source_revision=comparison_source_revision,
                    comparison_host_label=comparison_host_label,
                    comparison_ratio=comparison_ratio,
                    comparison_strict=comparison_strict,
                    comparison_required=comparison_required,
                    raw_evidence_directory=args.raw_evidence_dir,
                    peer_wire_security=peer_wire_security,
                    benchmark_target=benchmark_target,
                    preflight_failure_code=preflight_failure_code,
                    preflight_failure=preflight_failure,
                )
        codes.append(code)
        if transport == "uds" and frames_per_connection == 1 and code == 0:
            payload = json.loads(evidence.read_text(encoding="utf-8"))
            uds_one_frame_median = validated_profile_median(payload, "current UDS evidence")
        print(f"{'PASS' if code == 0 else 'FAIL'} admission-capacity evidence: {evidence}")
    return 0 if all(code == 0 for code in codes) else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"capacity smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

#!/usr/bin/env python3
"""Prove the public local ATM admission boundary accepts 1,000 writes/second.

This runner is deliberately a *clean-user* smoke gate. ADR-026 makes the
daemon and its SQLite store OS-user-owned, not ``ATM_HOME``-owned. It therefore
refuses to run beside an ambient daemon unless an authorized operator opts into
the explicit daemon-switch backup/restore lifecycle; changing ``ATM_HOME``
alone would not isolate a developer's real mail database.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import closing
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
from queue import Empty, Queue
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
from threading import Lock, Thread
import time
from typing import Any, Callable

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
HOOK_MODES = ("active", "disabled")
BENCHMARK_OBSERVABILITY_ERROR = "ATM_OBSERVABILITY_HEALTH_FAILED"
DAEMON_SWITCH = ROOT / ".claude" / "skills" / "daemon-switch" / "scripts" / "daemon-switch.py"
# The daemon-switch control plane can legitimately wait through its documented
# launchctl unload/owner-repair windows (up to 20s + two 20x2s polls).  Its
# outer timeout must cover that bounded recovery path; otherwise the runner
# reports a false benchmark failure while the switch is still repairing the
# selected singleton.
MANAGED_DAEMON_TIMEOUT_SECONDS = 120.0
DIAGNOSTIC_SAMPLE_COUNT = 3
DIAGNOSTIC_DURATION_SECONDS = 3.0
DIRECT_STORAGE_DIAGNOSTIC_WRITES = 10_000


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


@dataclass
class HostStateBackup:
    """Temporarily replace one idle host's complete ATM state with an empty root."""

    state_root: Path
    backup_root: Path | None

    @classmethod
    def begin(cls) -> "HostStateBackup":
        state_root = os_account_home() / ".atm"
        backup_root = state_root.with_name(
            f".atm-capacity-backup-{os.getpid()}-{time.monotonic_ns()}"
        )
        try:
            if state_root.exists():
                state_root.rename(backup_root)
            state_root.mkdir(mode=0o700, parents=True, exist_ok=False)
        except OSError:
            if backup_root.exists() and not state_root.exists():
                backup_root.rename(state_root)
            raise
        return cls(state_root, backup_root if backup_root.exists() else None)

    def restore(self) -> None:
        if self.state_root.exists():
            shutil.rmtree(self.state_root)
        if self.backup_root is not None:
            self.backup_root.rename(self.state_root)


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


@dataclass
class ManagedDaemonLifecycle:
    """Quiesce, isolate, and recover one explicitly authorized daemon pair.

    The state root is moved only after daemon-switch has stopped the managed
    daemon, so an open SQLite connection cannot race the snapshot. The selected
    pair is captured before quiescence and compared after restart; this flow
    never changes CLI/daemon selectors or their configuration.
    """

    options: ManagedDaemonOptions
    backup: HostStateBackup | None = None
    pre_pair: dict[str, str | None] | None = None
    quiesced: bool = False

    def begin(self) -> None:
        before = daemon_switch_result("status", self.options, doctor=True)
        self.pre_pair = selected_pair(before)
        daemon_switch_result("quiesce", self.options)
        self.quiesced = True
        try:
            require_clean_host_daemon_state(smoke_label="admission-capacity smoke")
            self.backup = HostStateBackup.begin()
        except Exception as error:
            recovery_error = self._restart_and_verify()
            if recovery_error is not None:
                raise SmokeError(
                    f"could not isolate managed daemon state: {error}; recovery also failed: {recovery_error}"
                ) from error
            raise

    def restore(self) -> None:
        if not self.quiesced:
            return
        failures: list[str] = []
        if self.backup is not None:
            try:
                self.backup.restore()
            except OSError as error:
                failures.append(f"could not restore prior host ATM state: {error}")
        recovery_error = self._restart_and_verify()
        if recovery_error is not None:
            failures.append(f"could not restore managed daemon pair: {recovery_error}")
        if failures:
            raise SmokeError("; ".join(failures))

    def _restart_and_verify(self) -> Exception | None:
        try:
            daemon_switch_result("restart", self.options)
            after = daemon_switch_result("status", self.options, doctor=True)
            if self.pre_pair is not None and selected_pair(after) != self.pre_pair:
                raise SmokeError("managed daemon selectors changed during capacity backup/restore")
            self.quiesced = False
            return None
        except Exception as error:  # pragma: no cover - covered through callers' recovery paths.
            return error


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


def select_host_state_isolation() -> str:
    """Require either a clean OS user or explicit backup/restore authority."""
    if os.environ.get("ATM_CAPACITY_ISOLATED_OS_USER") == "1":
        return "isolated_os_user"
    if os.environ.get("ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE") == "1":
        return "backup_restore"
    raise SmokeError(
        "set ATM_CAPACITY_ISOLATED_OS_USER=1 in a dedicated clean OS-user environment, "
        "or ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1 to back up and restore the idle host state"
    )


def reap_owned_daemon(process: subprocess.Popen[str]) -> None:
    """Terminate and reap the benchmark-owned child without mistaking a zombie for a leak."""
    terminate_process(process.pid)
    process.wait(timeout=10.0)


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


def validate_hook_mode(value: str) -> str:
    if value not in HOOK_MODES:
        raise SmokeError(f"benchmark hook mode must be one of {HOOK_MODES}")
    return value


def runtime_environment(atm_home: Path) -> dict[str, str]:
    environment = dict(os.environ)
    environment.update(
        {
            "ATM_HOME": str(atm_home),
            "ATM_IDENTITY": "capacity-agent",
            "ATM_TEAM": "capacity-team",
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

    The dedicated benchmark binary deliberately installs ``NullObservability``
    so a throughput run cannot create external hook/logging work.  Doctor
    consequently returns its one documented observability finding with a
    non-zero exit status even though the Tokio runtime is live and ready.  Do
    not turn that intentional harness configuration into a false capacity
    failure, but reject every other unhealthy response.
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

    findings = payload.get("findings")
    if (
        result.get("exit_code") == 1
        and summary.get("status") == "error"
        and isinstance(findings, list)
        and len(findings) == 1
        and isinstance(findings[0], dict)
        and findings[0].get("code") == "ATM_OBSERVABILITY_HEALTH_FAILED"
    ):
        return payload

    detail = result.get("stderr")
    if not isinstance(detail, str) or not detail.strip():
        detail = f"summary status {summary.get('status')!r}"
    raise SmokeError(f"capacity doctor failed: {detail.strip()}")


def parse_capacity_doctor(result: dict[str, Any], stage: str) -> dict[str, Any]:
    """Preserve the AL.14 benchmark-doctor contract for reusable diagnostics.

    The capacity runner uses :func:`benchmark_doctor_payload`, which additionally
    requires liveness and a complete summary.  This narrower helper remains the
    public AL.14 test seam: it accepts only the benchmark daemon's one expected
    null-observability finding once readiness is explicit.
    """
    try:
        payload = json.loads(result.get("stdout", ""))
    except json.JSONDecodeError as error:
        detail = result.get("stderr", "").strip() or str(error)
        raise SmokeError(f"{stage} returned invalid JSON: {detail}") from error
    if not isinstance(payload, dict):
        raise SmokeError(f"{stage} returned a non-object JSON payload")

    readiness = payload.get("runtime_status", {}).get("readiness")
    if readiness != "ready":
        summary = payload.get("summary", {}).get("message", "unknown readiness")
        raise SmokeError(f"{stage} failed: readiness={readiness!r}; {summary}")
    if result.get("exit_code") == 0:
        return payload

    findings = payload.get("findings", [])
    benchmark_only = (
        isinstance(findings, list)
        and len(findings) == 1
        and isinstance(findings[0], dict)
        and findings[0].get("severity") == "error"
        and findings[0].get("code") == BENCHMARK_OBSERVABILITY_ERROR
        and "observability adapter is not configured" in findings[0].get("message", "")
    )
    if benchmark_only:
        return payload

    detail = result.get("stderr", "").strip() or payload.get("summary", {}).get(
        "message", "unknown error",
    )
    raise SmokeError(f"{stage} failed: {detail}")


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
    daemon: Path, home: Path, env: dict[str, str], hook_mode: str,
) -> tuple[subprocess.Popen[str], DaemonOutputCapture]:
    """Start and await the feature-gated benchmark daemon with one hook mode."""
    hook_mode = validate_hook_mode(hook_mode)
    process = subprocess.Popen(
        [str(daemon), "--hook-mode", hook_mode], cwd=home, env=env,
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


def prepare_capacity_roster(atm: Path, env: dict[str, str], home: Path) -> None:
    """Create separate public-write and decomposition-only benchmark rosters."""
    for team, member in (
        ("capacity-team", "capacity-agent"),
        ("capacity-team", "capacity-recipient"),
        # The direct canonical-core probe must never add rows to the public
        # write profile's durability-count mailbox.
        ("capacity-core-team", "capacity-core-agent"),
        ("capacity-core-team", "capacity-core-recipient"),
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


def http_request_body(home: Path, sequence: int) -> bytes:
    """Build the documented /v1/atm/messages request; no dispatcher shortcut."""
    payload = {
        "home_dir": str(home),
        "current_dir": str(home),
        "caller_identity": "capacity-agent",
        "caller_team": "capacity-team",
        "to": {"agent": "capacity-recipient", "team": "capacity-team"},
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


def cached_roster_heartbeat_body(sequence: int) -> bytes:
    """Build a heartbeat that validates against the warmed roster snapshot.

    The daemon's heartbeat route calls ``LocalServiceRuntime.load_roster_member``.
    That method reads SQLite only for the first request of a team and serves its
    immutable in-process snapshot thereafter.  The benchmark explicitly warms
    that first request before recording these samples.
    """
    payload = {
        "team": "capacity-team",
        "member": "capacity-agent",
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
) -> dict[str, Any]:
    """Collect at least ten independent intervals over one sustained profile."""
    if sample_count <= 0:
        raise SmokeError("capacity sample count must be positive")
    if target_duration_seconds <= 0:
        raise SmokeError("capacity target duration must be positive")

    def submit(sequence: int, message_count: int) -> list[AdmissionResult]:
        if operation == "write":
            requests = [
                HttpRequest("/v1/atm/messages", http_request_body(home, sequence + offset), 201)
                for offset in range(message_count)
            ]
        elif operation == "cached_roster_heartbeat":
            requests = [
                HttpRequest(
                    "/v1/atm/heartbeat",
                    cached_roster_heartbeat_body(sequence + offset),
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
        "recipient": "capacity-recipient@capacity-team",
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
) -> dict[str, Any]:
    """Warm and then measure the public no-SQLite heartbeat route."""
    warmup = submit_connection(endpoint, [
        HttpRequest("/v1/atm/heartbeat", cached_roster_heartbeat_body(0), 200),
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
    )
    return {
        "route": "/v1/atm/heartbeat",
        "storage": "warmed LocalServiceRuntime roster snapshot; no SQLite reads after warmup",
        "warmup": {"status": warmup[0].status, "passed": True},
        "profile": profile,
    }


def run_direct_probe(
    benchmark_daemon: Path,
    environment: dict[str, str],
    workers: int,
    flag: str,
    kind: str,
) -> dict[str, Any]:
    """Run one isolated benchmark-binary decomposition mode."""
    result = command_result(
        [
            str(benchmark_daemon),
            flag,
            str(DIRECT_STORAGE_DIAGNOSTIC_WRITES),
            "--workers",
            str(workers),
        ],
        timeout=MANAGED_DAEMON_TIMEOUT_SECONDS,
        env=environment,
    )
    if result["exit_code"] != 0:
        detail = result["stderr"].strip() or result["stdout"].strip()
        raise SmokeError(f"direct {kind} probe failed: {detail}")
    lines = [line for line in result["stdout"].splitlines() if line.strip()]
    try:
        payload = json.loads(lines[-1])
    except (IndexError, json.JSONDecodeError) as error:
        raise SmokeError(f"direct {kind} probe returned no JSON result") from error
    if (
        not isinstance(payload, dict)
        or payload.get("kind") != kind
        or payload.get("requested_count") != DIRECT_STORAGE_DIAGNOSTIC_WRITES
        or payload.get("accepted_count") != DIRECT_STORAGE_DIAGNOSTIC_WRITES
        or not isinstance(payload.get("admissions_per_second"), (int, float))
        or payload["admissions_per_second"] <= 0
    ):
        raise SmokeError(f"direct {kind} probe returned an invalid result")
    return payload


def run_direct_storage_probe(
    benchmark_daemon: Path,
    environment: dict[str, str],
    workers: int,
) -> dict[str, Any]:
    """Measure only the Tokio async admission queue and its one SQLite writer."""
    return run_direct_probe(
        benchmark_daemon,
        environment,
        workers,
        "--direct-storage-admission",
        "async_storage_admission",
    )


def run_direct_core_write_probe(
    benchmark_daemon: Path,
    environment: dict[str, str],
    workers: int,
) -> dict[str, Any]:
    """Measure canonical write preparation through the async admission seam."""
    return run_direct_probe(
        benchmark_daemon,
        environment,
        workers,
        "--direct-core-write",
        "canonical_core_write",
    )


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


def verify_durable_admissions(db_path: Path, expected_count: int) -> dict[str, int | bool | str]:
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
                ("capacity-team", "capacity-recipient"),
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
) -> tuple[float, str]:
    """Use one complete accepted UDS revision when confirming Windows TCP."""
    by_revision: dict[str, dict[int, tuple[str, float]]] = {}
    for path in directory.glob("*.json"):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            candidate_revision = payload.get("source_revision")
            candidate_frame = payload.get("frames_per_connection")
            if (
                payload.get("host_label") == host_label
                and payload.get("transport") == transport
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
            "missing a complete passed UDS comparison set for host "
            f"{host_label} at or before source revision {revision}"
        )
    selected_revision, profiles = max(
        complete.items(),
        key=lambda item: max(generated for generated, _median in item[1].values()),
    )
    _generated_at, median = profiles[frames_per_connection]
    return median, selected_revision


def baseline_comparison_reference(path: Path | None) -> tuple[float | None, str | None]:
    """Return a durable baseline's median and source revision for public evidence."""
    reference = baseline_reference(path)
    if reference is None:
        return None, None
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
    hook_mode: str = "active",
    managed_daemon: ManagedDaemonOptions | None = None,
) -> tuple[int, Path]:
    """Start one branch daemon, exercise public UDS API, retain evidence, then clean up."""
    transport = validate_transport(transport)
    hook_mode = validate_hook_mode(hook_mode)
    if frames_per_connection not in SPARSE_FRAMES_PER_CONNECTION:
        raise SmokeError(f"frames per connection must be one of {SPARSE_FRAMES_PER_CONNECTION}")
    if requested_messages <= 0:
        raise SmokeError("requested messages must be positive")
    if workers <= 0:
        raise SmokeError("capacity worker limit must be positive")
    isolation_mode = select_host_state_isolation()
    home = validate_capacity_home(atm_home)
    if isolation_mode == "isolated_os_user":
        require_clean_host_daemon_state(smoke_label="admission-capacity smoke")
    elif managed_daemon is None:
        raise SmokeError(
            "ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1 requires the managed daemon "
            "service details; pass --managed-service and the platform-specific daemon-switch options"
        )
    atm = release_binary("atm")
    daemon = release_binary("atm-daemon-benchmark")
    env = runtime_environment(home)
    process: subprocess.Popen[str] | None = None
    daemon_output: DaemonOutputCapture | None = None
    managed_lifecycle: ManagedDaemonLifecycle | None = None
    before: list[int] | None = None
    evidence: dict[str, Any] = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "host_label": os.environ.get("ATM_CAPACITY_HOST_LABEL", "local"),
        "transport": transport,
        "hook_mode": hook_mode,
        "frames_per_connection": frames_per_connection,
        "run_duration_s": None,
        "messages_per_connection": frames_per_connection,
        "requested_messages_per_sample": requested_messages,
        "minimum_sample_count": sample_count,
        "sample_count": None,
        "target_duration_s": TARGET_PROFILE_DURATION_SECONDS,
        "worker_limit": workers,
        "source_revision": source_revision(),
        "release": {"atm": str(atm), "atm_daemon_benchmark": str(daemon)},
        "atm_home": str(home),
        "host_state_isolation": isolation_mode,
        "runs": [],
        "thresholds": None,
        "comparison_source_revision": comparison_source_revision,
        "comparison_host_label": comparison_host_label,
        "stages": {
            "runtime_view_validation": "daemon-owned; no peer/store/network work is requested by this client before response",
            "sqlite_transaction": "measured by each public admission response latency",
            "post_commit_received_hook": (
                "disabled by the feature-gated benchmark-only hook selector"
                if hook_mode == "disabled"
                else "awaited after durable write; any hook failure is returned as a successful write warning"
            ),
            "response_write": "included in each public admission response latency",
        },
        "decomposition": {},
    }
    try:
        if isolation_mode == "backup_restore":
            assert managed_daemon is not None
            managed_lifecycle = ManagedDaemonLifecycle(managed_daemon)
            managed_lifecycle.begin()
        before = count_atm_daemon_processes()
        home.mkdir(parents=True, exist_ok=False)
        prepare_capacity_roster(atm, env, home)
        evidence["decomposition"]["async_storage_admission"] = run_direct_storage_probe(
            daemon,
            env,
            workers,
        )
        evidence["decomposition"]["canonical_core_write"] = run_direct_core_write_probe(
            daemon,
            env,
            workers,
        )
        process, daemon_output = start_capacity_daemon(daemon, home, env, hook_mode)
        doctor = command_result(
            [str(atm), "doctor", "--json"],
            timeout=10.0,
            env=host_runtime_client_environment(env),
        )
        doctor_payload = benchmark_doctor_payload(doctor)
        evidence["daemon_pid"] = process.pid
        evidence["doctor"] = doctor_payload
        evidence["doctor_status"] = "passed"
        endpoint = local_endpoint(transport)
        evidence["endpoint"] = {
            "transport": endpoint.kind,
            "address": endpoint.address,
        }
        if endpoint.kind == "uds" and not Path(str(endpoint.address)).exists():
            raise SmokeError(f"daemon did not publish public local socket {endpoint.address}")
        evidence["decomposition"]["cached_roster_heartbeat"] = run_cached_roster_heartbeat_probe(
            endpoint,
            home,
            frames_per_connection,
            workers,
        )
        profile = run_profile(
            endpoint,
            home,
            frames_per_connection,
            requested_messages,
            sample_count,
            workers,
        )
        evidence["runs"] = [profile]
        evidence["sample_count"] = profile["sample_count"]
        evidence["target_duration_s"] = profile["target_duration_s"]
        baseline_median = load_baseline_median(
            baseline_path, transport, frames_per_connection,
        )
        evidence["baseline"] = baseline_reference(baseline_path)
        evidence["thresholds"] = evaluate_profile_thresholds(
            profile, baseline_median, comparison_median, comparison_ratio,
            comparison_strict, comparison_required,
        )
        evidence["run_duration_s"] = profile["run_duration_s"]
        evidence["passed"] = evidence["thresholds"]["passed"]
        expected_accepted_count = sum(item["accepted_count"] for item in profile["intervals"])

        # This intentionally restarts the same isolated daemon.  A transport
        # success alone is not durable evidence, so prove every committed row
        # survived using an exact read-only count of its disposable store.
        reap_owned_daemon(process)
        daemon_output.join()
        evidence["pre_restart_daemon_output"] = daemon_output.evidence()
        process = None
        daemon_output = None
        process, daemon_output = start_capacity_daemon(daemon, home, env, hook_mode)
        restart_doctor = command_result(
            [str(atm), "doctor", "--json"],
            timeout=10.0,
            env=host_runtime_client_environment(env),
        )
        benchmark_doctor_payload(restart_doctor)
        # The full doctor payload is host-private diagnostics; publication only
        # needs the asserted healthy result after the restart.
        evidence["doctor_after_restart"] = {"status": "passed"}
        evidence["durability_after_restart"] = verify_durable_admissions(
            os_account_home() / ".atm" / "db" / "mail.db", expected_accepted_count,
        )
    except (OSError, ValueError, SmokeError) as error:
        evidence["passed"] = False
        evidence["failure"] = str(error)
    finally:
        if process is not None:
            try:
                reap_owned_daemon(process)
            except subprocess.TimeoutExpired:
                evidence["passed"] = False
                evidence["cleanup_failure"] = (
                    f"admission-capacity daemon pid {process.pid} did not exit within 10.0s"
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
        if managed_lifecycle is not None:
            try:
                managed_lifecycle.restore()
                evidence["managed_daemon_recovery"] = "doctor-verified"
            except SmokeError as error:
                evidence["passed"] = False
                evidence["managed_daemon_recovery"] = "failed"
                evidence["cleanup_failure"] = str(error)
        raw_evidence_path = write_raw_evidence(raw_evidence_directory, evidence)
        evidence_path = write_evidence(evidence_directory, evidence)
        print(f"local benchmark trace: {raw_evidence_path}")
        try:
            if home.exists():
                for child in sorted(home.rglob("*"), reverse=True):
                    if child.is_file() or child.is_symlink():
                        child.unlink()
                    elif child.is_dir():
                        child.rmdir()
                home.rmdir()
        except OSError as error:
            raise SmokeError(f"could not remove temporary capacity ATM_HOME {home}: {error}") from error
    return (0 if evidence.get("passed") else 1), evidence_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
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
    parser.add_argument("--transport", default="uds" if os.name != "nt" else "tcp")
    parser.add_argument(
        "--hook-mode",
        default="active",
        choices=HOOK_MODES,
        help="measure the replacement received hook as active or benchmark-authorized disabled",
    )
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
    parser.add_argument(
        "--managed-service",
        help=(
            "daemon-switch service label for explicit ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1 "
            "mode; never needed for an isolated OS user"
        ),
    )
    parser.add_argument(
        "--managed-launch-agent-plist",
        type=Path,
        help="macOS LaunchAgent plist required by daemon-switch backup/restore mode",
    )
    parser.add_argument(
        "--managed-cli-link",
        type=Path,
        help="optional selected atm CLI symlink passed through to daemon-switch",
    )
    parser.add_argument(
        "--managed-daemon-link",
        type=Path,
        help="optional selected atm-daemon symlink passed through to daemon-switch",
    )
    parser.add_argument(
        "--managed-repair-orphan",
        action="store_true",
        help="allow daemon-switch's narrow verified-orphan repair during controlled lifecycle recovery",
    )
    args = parser.parse_args()
    transport = validate_transport(args.transport)
    hook_mode = validate_hook_mode(args.hook_mode)
    sparse_profiles = tuple(args.frames_per_connection or SPARSE_FRAMES_PER_CONNECTION)
    sustained_profiles = tuple(args.sustained or ())
    profiles = selected_profiles(sparse_profiles, sustained_profiles)
    managed_daemon = (
        ManagedDaemonOptions(
            service=args.managed_service,
            launch_agent_plist=args.managed_launch_agent_plist,
            cli_link=args.managed_cli_link,
            daemon_link=args.managed_daemon_link,
            repair_orphan=args.managed_repair_orphan,
        )
        if args.managed_service
        else None
    )
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
                    args.evidence_dir, comparison_host_label, "uds", frames_per_connection,
                    current_revision,
                )
            except SmokeError:
                if comparison_required:
                    raise
        if args.atm_home is None:
            with tempfile.TemporaryDirectory(prefix="atm-capacity-parent-") as temp:
                home = Path(temp) / f"{CAPACITY_ROOT_PREFIX}{position}"
                code, evidence = run_capacity(
                    home, args.evidence_dir, args.transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    baseline_path=profile_baseline,
                    comparison_median=comparison_median,
                    comparison_source_revision=comparison_source_revision,
                    comparison_host_label=comparison_host_label,
                    comparison_ratio=comparison_ratio,
                    comparison_strict=comparison_strict,
                    comparison_required=comparison_required,
                    raw_evidence_directory=args.raw_evidence_dir,
                    hook_mode=hook_mode,
                    managed_daemon=managed_daemon,
                )
        else:
            home = args.atm_home / f"{CAPACITY_ROOT_PREFIX}{position}"
            code, evidence = run_capacity(
                    home, args.evidence_dir, args.transport,
                    frames_per_connection, requested_messages, workers=args.workers,
                    baseline_path=profile_baseline,
                    comparison_median=comparison_median,
                    comparison_source_revision=comparison_source_revision,
                    comparison_host_label=comparison_host_label,
                    comparison_ratio=comparison_ratio,
                    comparison_strict=comparison_strict,
                    comparison_required=comparison_required,
                    raw_evidence_directory=args.raw_evidence_dir,
                    hook_mode=hook_mode,
                    managed_daemon=managed_daemon,
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

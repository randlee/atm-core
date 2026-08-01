#!/usr/bin/env python3
"""Prove the public local ATM admission boundary accepts 1,000 writes/second.

This runner is deliberately a *clean-user* smoke gate.  ADR-026 makes the
daemon and its SQLite store OS-user-owned, not ``ATM_HOME``-owned.  Therefore
it refuses to run beside an ambient daemon and requires an explicit isolated
OS-user acknowledgement; changing ``ATM_HOME`` alone would not isolate a
developer's real mail database.
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

if os.name != "nt":
    import pwd

from daemon_lifecycle import (
    assert_no_process_leak,
    count_atm_daemon_processes,
    require_clean_host_daemon_state,
    terminate_process,
)
from smoke_common import SmokeError, command_result
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
TCP_COMPARISON_FRAMES = (1, 2, 8, 16, 64)
SUSTAINED_MESSAGE_COUNTS = (10_000, 100_000)
DAEMON_OUTPUT_TAIL_LINES = 200
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")


@dataclass(frozen=True)
class AdmissionResult:
    status: int
    elapsed_ms: float
    failure: str | None = None
    request_bytes: int = 0
    response_bytes: int = 0
    response_summary: str | None = None


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
    daemon: Path, home: Path, env: dict[str, str],
) -> tuple[subprocess.Popen[str], DaemonOutputCapture]:
    """Start and await exactly one benchmark-owned daemon process."""
    process = subprocess.Popen(
        [str(daemon)], cwd=home, env=env,
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
    """Create the sender and a distinct local durable-write recipient."""
    for member in ("capacity-agent", "capacity-recipient"):
        result = command_result(
            [str(atm), "teams", "add-member", "capacity-team", member, "--home-dir", str(home), "--json"],
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


def submit_connection(endpoint: LocalEndpoint, bodies: list[bytes]) -> list[AdmissionResult]:
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
            requests = []
            for index, body in enumerate(bodies):
                connection = "close" if index + 1 == len(bodies) else "keep-alive"
                requests.append(
                    b"POST /v1/atm/messages HTTP/1.1\r\n"
                    b"Content-Type: application/json\r\n"
                    + capability
                    + f"Content-Length: {len(body)}\r\nConnection: {connection}\r\n\r\n".encode("ascii")
                    + body
                )

            response_buffer = bytearray()
            for start in range(0, len(requests), MAX_IN_FLIGHT_REQUESTS):
                batch = requests[start:start + MAX_IN_FLIGHT_REQUESTS]
                request_started = time.perf_counter()
                stream.sendall(b"".join(batch))
                for request in batch:
                    status, response_bytes, response_summary = read_http_response(stream, response_buffer)
                    results.append(AdmissionResult(
                        status=status,
                        elapsed_ms=(time.perf_counter() - request_started) * 1_000,
                        failure=None if status == 201 else f"HTTP {status}: {response_summary or 'no response body'}",
                        request_bytes=len(request),
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
    accepted = sum(result.status == 201 for result in results)
    failures = [result.failure or f"HTTP {result.status}" for result in results if result.status != 201]
    latencies = [result.elapsed_ms for result in results]
    latency_distribution = distribution(latencies) if latencies else {
        "min": 0.0,
        "p50": 0.0,
        "p95": 0.0,
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
        "passed": error_free and elapsed_seconds <= requested_messages / 1_000,
    }


def run_profile(
    endpoint: LocalEndpoint,
    home: Path,
    frames_per_connection: int,
    requested_messages: int,
    sample_count: int,
    workers: int,
    target_duration_seconds: float = TARGET_PROFILE_DURATION_SECONDS,
) -> dict[str, Any]:
    """Collect at least ten independent intervals over one sustained profile."""
    if sample_count <= 0:
        raise SmokeError("capacity sample count must be positive")
    if target_duration_seconds <= 0:
        raise SmokeError("capacity target duration must be positive")

    def submit(sequence: int, message_count: int) -> list[AdmissionResult]:
        return submit_connection(endpoint, [
            http_request_body(home, sequence + offset)
            for offset in range(message_count)
        ])

    intervals: list[dict[str, Any]] = []
    elapsed_seconds = 0.0
    while len(intervals) < sample_count or elapsed_seconds < target_duration_seconds:
        interval = run_interval(
            submit, len(intervals), frames_per_connection, workers, requested_messages,
        )
        intervals.append(interval)
        elapsed_seconds += float(interval["elapsed_seconds"])
        # Retain clean under-threshold intervals so the report can distinguish a
        # throughput plateau from an actual request/response failure.
        if not interval.get("error_free", interval["passed"]):
            break
    return {
        "recipient": "capacity-recipient@capacity-team",
        "requested_messages_per_sample": requested_messages,
        "minimum_sample_count": sample_count,
        "sample_count": len(intervals),
        "target_duration_s": target_duration_seconds,
        "run_duration_s": elapsed_seconds,
        "intervals": intervals,
        "passed": all(item["passed"] for item in intervals),
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


def profile_median_admissions_per_second(profile: dict[str, Any]) -> float:
    """Return the midpoint rate retained by a complete profile."""
    rates = [float(item["admissions_per_second"]) for item in profile["intervals"]]
    return distribution(rates)["p50"] if rates else 0.0


def evaluate_profile_thresholds(
    profile: dict[str, Any], baseline_median: float | None,
    comparison_median: float | None = None,
    comparison_ratio: float = 1.0,
    comparison_strict: bool = False,
    comparison_required: bool = True,
) -> dict[str, Any]:
    """Make the admission, baseline, and transport-comparison gates explicit."""
    median = profile_median_admissions_per_second(profile)
    admission_passed = all(item["passed"] for item in profile["intervals"])
    baseline_passed = baseline_median is None or median >= baseline_median
    comparison_target = None if comparison_median is None else comparison_median * comparison_ratio
    comparison_passed = (
        comparison_target is None
        or (median > comparison_target if comparison_strict else median >= comparison_target)
    )
    return {
        "admissions_per_second_minimum": 1_000,
        "median_admissions_per_second": median,
        "baseline_median_admissions_per_second": baseline_median,
        "admission_passed": admission_passed,
        "baseline_passed": baseline_passed,
        "comparison_median_admissions_per_second": comparison_median,
        "comparison_ratio": comparison_ratio if comparison_median is not None else None,
        "comparison_target_admissions_per_second": comparison_target,
        "comparison_strict": comparison_strict if comparison_median is not None else None,
        "comparison_required": comparison_required if comparison_median is not None else None,
        "comparison_passed": comparison_passed,
        "passed": admission_passed and baseline_passed and (
            comparison_passed if comparison_required else True
        ),
    }


def load_baseline_median(
    path: Path | None, transport: str, frames_per_connection: int,
) -> float | None:
    """Read a prior compatible one-profile evidence artifact when requested."""
    if path is None:
        return None
    try:
        baseline = json.loads(path.read_text(encoding="utf-8"))
        if baseline["transport"] != transport:
            raise SmokeError(
                f"capacity baseline transport {baseline['transport']!r} does not match {transport!r}"
            )
        if baseline["frames_per_connection"] != frames_per_connection:
            raise SmokeError(
                "capacity baseline frames_per_connection does not match the selected profile"
            )
        return validated_profile_median(baseline, "capacity baseline")
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SmokeError(f"could not read admission-capacity baseline {path}: {error}") from error


def validated_profile_median(payload: dict[str, Any], label: str) -> float:
    """Return a median only from evidence that passed its own methodology gates."""
    if not payload.get("passed", False):
        raise SmokeError(f"{label} did not pass its own acceptance gates")
    if payload.get("sample_count", 0) < payload.get("minimum_sample_count", 10):
        raise SmokeError(f"{label} has fewer than its required samples")
    if payload.get("run_duration_s", 0.0) < payload.get("target_duration_s", 20.0):
        raise SmokeError(f"{label} did not run for its required duration")
    return recorded_profile_median(payload, label)


def recorded_profile_median(payload: dict[str, Any], label: str) -> float:
    """Read the recorded median without treating a failed run as a baseline."""
    try:
        if payload.get("schema_version") == 3:
            return float(payload["metrics"]["admissions_per_second"]["p50"])
        return profile_median_admissions_per_second(payload["runs"][0])
    except (KeyError, TypeError, ValueError, IndexError) as error:
        raise SmokeError(f"invalid {label}") from error


def baseline_reference(path: Path | None) -> dict[str, Any] | None:
    """Retain the comparison artifact identity alongside its measured median."""
    if path is None:
        return None
    try:
        baseline = json.loads(path.read_text(encoding="utf-8"))
        return {
            "source_revision": baseline.get("source_revision"),
            "generated_at": baseline.get("generated_at"),
            "run_duration_s": baseline.get("run_duration_s"),
            "passed": bool(baseline.get("passed", False)),
            "median_admissions_per_second": recorded_profile_median(baseline, "capacity baseline"),
        }
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SmokeError(f"could not describe admission-capacity baseline {path}: {error}") from error


def matching_profile_median(
    directory: Path, host_label: str, transport: str, frames_per_connection: int,
    revision: str,
) -> float:
    """Load this build's retained reference profile, never an arbitrary old run."""
    candidates: list[tuple[str, float]] = []
    for path in directory.glob("*.json"):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
            if (
                payload.get("host_label") == host_label
                and payload.get("transport") == transport
                and payload.get("frames_per_connection") == frames_per_connection
                and payload.get("source_revision") == revision
            ):
                candidates.append((
                    str(payload.get("generated_at", "")),
                    validated_profile_median(payload, "comparison evidence"),
                ))
        except (OSError, TypeError, ValueError, json.JSONDecodeError, SmokeError):
            continue
    if not candidates:
        raise SmokeError(
            f"missing {transport} f{frames_per_connection} comparison evidence "
            f"for host {host_label} at source revision {revision}"
        )
    _, median = max(candidates, key=lambda candidate: candidate[0])
    return median


def matching_profile_reference(
    directory: Path,
    host_label: str,
    transport: str,
    frames_per_connection: int,
    revision: str,
) -> tuple[float, str]:
    """Use one complete accepted UDS revision when confirming Windows TCP.

    The Windows confirmation branch necessarily contains commits after the
    Mac AI.40 run.  A comparison is valid only when every requested sparse
    profile comes from the same passed revision, and that revision is an
    ancestor of the build being measured.  This avoids both a false exact-tip
    prerequisite failure and mixing metrics from unrelated Mac builds.
    """
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
) -> tuple[int, Path]:
    """Start one branch daemon, exercise public UDS API, retain evidence, then clean up."""
    transport = validate_transport(transport)
    if frames_per_connection not in SPARSE_FRAMES_PER_CONNECTION:
        raise SmokeError(f"frames per connection must be one of {SPARSE_FRAMES_PER_CONNECTION}")
    if requested_messages <= 0:
        raise SmokeError("requested messages must be positive")
    if workers <= 0:
        raise SmokeError("capacity worker limit must be positive")
    isolation_mode = select_host_state_isolation()
    home = validate_capacity_home(atm_home)
    require_clean_host_daemon_state(smoke_label="admission-capacity smoke")
    before = count_atm_daemon_processes()
    atm = release_binary("atm")
    daemon = release_binary("atm-daemon")
    home.mkdir(parents=True, exist_ok=False)
    env = runtime_environment(home)
    process: subprocess.Popen[str] | None = None
    daemon_output: DaemonOutputCapture | None = None
    host_state_backup: HostStateBackup | None = None
    evidence: dict[str, Any] = {
        "schema_version": 2,
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "host_label": os.environ.get("ATM_CAPACITY_HOST_LABEL", "local"),
        "transport": transport,
        "frames_per_connection": frames_per_connection,
        "run_duration_s": None,
        "messages_per_connection": frames_per_connection,
        "requested_messages_per_sample": requested_messages,
        "minimum_sample_count": sample_count,
        "sample_count": None,
        "target_duration_s": TARGET_PROFILE_DURATION_SECONDS,
        "worker_limit": workers,
        "source_revision": source_revision(),
        "release": {"atm": str(atm), "atm_daemon": str(daemon)},
        "atm_home": str(home),
        "host_state_isolation": isolation_mode,
        "runs": [],
        "thresholds": None,
        "comparison_source_revision": comparison_source_revision,
        "comparison_host_label": comparison_host_label,
        "stages": {
            "runtime_view_validation": "daemon-owned; no peer/store/network work is requested by this client before response",
            "sqlite_transaction": "measured by each public admission response latency",
            "post_commit_signal": "not awaited by this runner",
            "response_write": "included in each public admission response latency",
        },
    }
    try:
        if isolation_mode == "backup_restore":
            host_state_backup = HostStateBackup.begin()
        prepare_capacity_roster(atm, env, home)
        process, daemon_output = start_capacity_daemon(daemon, home, env)
        doctor = command_result([str(atm), "doctor", "--json"], timeout=10.0, env=env)
        if doctor["exit_code"] != 0:
            raise SmokeError(f"capacity doctor failed: {doctor['stderr'].strip()}")
        evidence["daemon_pid"] = process.pid
        evidence["doctor"] = json.loads(doctor["stdout"])
        evidence["doctor_status"] = "passed"
        endpoint = local_endpoint(transport)
        evidence["endpoint"] = {
            "transport": endpoint.kind,
            "address": endpoint.address,
        }
        if endpoint.kind == "uds" and not Path(str(endpoint.address)).exists():
            raise SmokeError(f"daemon did not publish public local socket {endpoint.address}")
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
        process, daemon_output = start_capacity_daemon(daemon, home, env)
        restart_doctor = command_result([str(atm), "doctor", "--json"], timeout=10.0, env=env)
        if restart_doctor["exit_code"] != 0:
            raise SmokeError(f"capacity doctor after restart failed: {restart_doctor['stderr'].strip()}")
        # The full doctor payload is host-private diagnostics; publication only
        # needs the asserted healthy result after the restart.
        json.loads(restart_doctor["stdout"])
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
        try:
            assert_no_process_leak(before, count_atm_daemon_processes(), smoke_label="admission-capacity smoke")
        except RuntimeError as error:
            evidence["passed"] = False
            evidence["cleanup_failure"] = str(error)
        if host_state_backup is not None:
            try:
                host_state_backup.restore()
            except OSError as error:
                evidence["passed"] = False
                evidence["cleanup_failure"] = f"could not restore prior host ATM state: {error}"
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
    transport = validate_transport(args.transport)
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
        profile_baseline = None
        if transport == "uds":
            if frames_per_connection == 1:
                profile_baseline = args.baseline
            else:
                if uds_one_frame_median is None:
                    raise SmokeError("UDS multi-frame profile requires the current UDS one-frame reference")
                comparison_median = uds_one_frame_median
                comparison_strict = True
        else:
            comparison_median, comparison_source_revision = matching_profile_reference(
                args.evidence_dir, comparison_host_label, "uds", frames_per_connection, current_revision,
            )
            # Connection setup dominates one/two-frame TCP.  Keep an explicit
            # short-frame floor instead of hiding it, while retaining the
            # stricter batching-parity floor where frames amortize setup.
            comparison_ratio = 0.9 if frames_per_connection >= 8 else 0.75
            comparison_required = os.name != "nt"
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

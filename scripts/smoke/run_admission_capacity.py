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
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
from queue import Empty, Queue
import socket
import subprocess
import sys
import tempfile
from threading import Thread
import time
from typing import Any, Callable

if os.name != "nt":
    import pwd

from daemon_lifecycle import (
    assert_no_process_leak,
    count_atm_daemon_processes,
    require_clean_host_daemon_state,
    terminate_process,
    wait_for_process_exit,
)
from fixtures import release_binary
from smoke_common import SmokeError, command_result, sanitize


ROOT = Path(__file__).resolve().parents[2]
INTERVALS = 10
ADMISSIONS_PER_INTERVAL = 1_000
WORKERS = 64
READY_TIMEOUT_SECONDS = 30.0
CAPACITY_ROOT_PREFIX = "atm-capacity-"
MAX_ERROR_RESPONSE_BYTES = 8_192


@dataclass(frozen=True)
class AdmissionResult:
    status: int
    elapsed_ms: float
    failure: str | None = None


@dataclass(frozen=True)
class HttpResponseSummary:
    """Bounded public HTTP response evidence for one admission."""

    status: int
    failure: str | None = None


@dataclass(frozen=True)
class LocalEndpoint:
    """One authenticated public local daemon endpoint."""

    kind: str
    address: str | tuple[str, int]
    capability: str | None = None


def require_isolated_os_user() -> None:
    """Reject a developer's ordinary account before any daemon can start."""
    if os.environ.get("ATM_CAPACITY_ISOLATED_OS_USER") != "1":
        raise SmokeError(
            "set ATM_CAPACITY_ISOLATED_OS_USER=1 only in a dedicated clean OS-user environment; "
            "ADR-026 forbids treating ATM_HOME as an isolated database"
        )


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


def await_daemon_ready(process: subprocess.Popen[str]) -> None:
    """Wait only for the daemon's explicit readiness signal."""
    if process.stdout is None:
        raise SmokeError("capacity daemon stdout was not captured")
    lines: Queue[str | None] = Queue()

    def read_stdout() -> None:
        try:
            for line in process.stdout:
                lines.put(line)
        finally:
            lines.put(None)

    Thread(target=read_stdout, name="atm-capacity-ready", daemon=True).start()
    deadline = time.monotonic() + READY_TIMEOUT_SECONDS
    last_line = ""
    while time.monotonic() < deadline:
        try:
            line = lines.get(timeout=min(0.1, max(0.0, deadline - time.monotonic())))
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


def prepare_capacity_roster(atm: Path, env: dict[str, str], home: Path) -> None:
    """Create the sole local recipient through the public CLI boundary."""
    result = command_result(
        [str(atm), "teams", "add-member", "capacity-team", "capacity-agent", "--home-dir", str(home), "--json"],
        timeout=15.0,
        env=env,
    )
    if result["exit_code"] != 0:
        raise SmokeError(f"could not create capacity recipient: {result['stderr'].strip()}")


def configure_controlled_peer(atm: Path, env: dict[str, str], host: str, fingerprint: str) -> None:
    """Install one test-only trusted peer through the public CLI and reload path."""
    result = command_result(
        [
            str(atm), "peer", "trust", "add", "--host", host,
            "--fingerprint", fingerprint, "--yes",
        ],
        timeout=15.0,
        env=env,
    )
    if result["exit_code"] != 0:
        raise SmokeError(f"could not configure controlled peer {host}: {result['stderr'].strip()}")


def http_request_body(home: Path, sequence: int, peer_host: str) -> bytes:
    """Build the documented /v1/atm/messages request; no dispatcher shortcut."""
    payload = {
        "home_dir": str(home),
        "current_dir": str(home),
        "caller_identity": "capacity-agent",
        "caller_team": "capacity-team",
        "to": {"agent": "capacity-agent", "team": "capacity-team", "host": peer_host},
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


def local_endpoint() -> LocalEndpoint:
    """Resolve the documented UDS/TCP public API without a dispatcher seam."""
    runtime = os_account_home() / ".atm" / "daemon"
    if os.name != "nt":
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


def read_http_response_summary(stream: socket.socket) -> HttpResponseSummary:
    """Retain a bounded structured error response instead of losing its cause."""
    data = bytearray()
    while b"\r\n\r\n" not in data:
        chunk = stream.recv(4096)
        if not chunk:
            raise SmokeError("daemon closed the local HTTP connection before response headers")
        data.extend(chunk)
        if len(data) > 16_384:
            raise SmokeError("daemon local HTTP response headers exceeded the safety bound")
    raw_headers, body = bytes(data).split(b"\r\n\r\n", 1)
    headers = raw_headers.decode("ascii", "replace").split("\r\n")
    status_line = headers[0]
    fields = status_line.split()
    if len(fields) < 2 or not fields[1].isdigit():
        raise SmokeError(f"daemon returned malformed HTTP status line: {status_line}")
    content_length = 0
    for header in headers[1:]:
        name, separator, value = header.partition(":")
        if separator and name.lower() == "content-length":
            try:
                content_length = int(value.strip())
            except ValueError as error:
                raise SmokeError("daemon returned invalid Content-Length") from error
            break
    if content_length < 0:
        raise SmokeError("daemon returned negative Content-Length")
    captured = bytearray(body[:MAX_ERROR_RESPONSE_BYTES])
    remaining = content_length - len(body)
    while remaining > 0:
        chunk = stream.recv(min(4096, remaining))
        if not chunk:
            raise SmokeError("daemon closed the local HTTP connection before its declared body")
        if len(captured) < MAX_ERROR_RESPONSE_BYTES:
            captured.extend(chunk[:MAX_ERROR_RESPONSE_BYTES - len(captured)])
        remaining -= len(chunk)
    status = int(fields[1])
    if status == 201:
        return HttpResponseSummary(status)
    try:
        error = json.loads(bytes(captured))
    except json.JSONDecodeError:
        detail = sanitize(bytes(captured).decode("utf-8", "replace"))
        return HttpResponseSummary(status, f"HTTP {status}: {detail or '<empty error response>'}")
    if isinstance(error, dict):
        code = error.get("code")
        message = error.get("message")
        if isinstance(code, str) and isinstance(message, str):
            return HttpResponseSummary(status, f"HTTP {status} {code}: {sanitize(message)}")
    return HttpResponseSummary(status, f"HTTP {status}: {sanitize(json.dumps(error, sort_keys=True))}")


def submit_admission(endpoint: LocalEndpoint, body: bytes) -> AdmissionResult:
    """Submit one real same-host API request over the daemon's public local transport."""
    started = time.perf_counter()
    capability = (
        f"X-ATM-Local-Capability: {endpoint.capability}\r\n".encode("ascii")
        if endpoint.capability is not None
        else b""
    )
    request = (
        b"POST /v1/atm/messages HTTP/1.1\r\n"
        b"Content-Type: application/json\r\n"
        + capability
        + f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode("ascii")
        + body
    )
    try:
        family = socket.AF_UNIX if endpoint.kind == "uds" else socket.AF_INET
        with socket.socket(family, socket.SOCK_STREAM) as stream:
            stream.settimeout(3.5)
            stream.connect(endpoint.address)
            stream.sendall(request)
            response = read_http_response_summary(stream)
        elapsed_ms = (time.perf_counter() - started) * 1_000
        return AdmissionResult(status=response.status, elapsed_ms=elapsed_ms, failure=response.failure)
    except (OSError, SmokeError) as error:
        return AdmissionResult(status=0, elapsed_ms=(time.perf_counter() - started) * 1_000, failure=str(error))


def run_interval(submit: Callable[[int], AdmissionResult], interval: int) -> dict[str, Any]:
    """Run one exactly-sized admission interval without retrying failed writes."""
    started = time.perf_counter()
    results: list[AdmissionResult] = []
    with ThreadPoolExecutor(max_workers=WORKERS, thread_name_prefix="atm-capacity") as executor:
        futures = [executor.submit(submit, interval * ADMISSIONS_PER_INTERVAL + sequence) for sequence in range(ADMISSIONS_PER_INTERVAL)]
        for future in as_completed(futures):
            results.append(future.result())
    elapsed_seconds = time.perf_counter() - started
    accepted = sum(result.status == 201 for result in results)
    failures = [result.failure or f"HTTP {result.status}" for result in results if result.status != 201]
    latencies = sorted(result.elapsed_ms for result in results)
    return {
        "interval": interval + 1,
        "accepted_count": accepted,
        "response_count": len(results),
        "elapsed_seconds": elapsed_seconds,
        "admissions_per_second": accepted / elapsed_seconds if elapsed_seconds else 0.0,
        "latency_ms": {
            "min": latencies[0] if latencies else 0.0,
            "p50": latencies[len(latencies) // 2] if latencies else 0.0,
            "max": latencies[-1] if latencies else 0.0,
        },
        "first_failure": failures[0] if failures else None,
        "response_timing_ms": {
            "min": latencies[0] if latencies else 0.0,
            "p50": latencies[len(latencies) // 2] if latencies else 0.0,
            "max": latencies[-1] if latencies else 0.0,
        },
        "passed": accepted == ADMISSIONS_PER_INTERVAL and elapsed_seconds <= 1.0,
    }


def run_peer_case(endpoint: LocalEndpoint, home: Path, peer_host: str, label: str) -> dict[str, Any]:
    """Collect all ten intervals for one configured peer state."""
    def submit(sequence: int) -> AdmissionResult:
        return submit_admission(endpoint, http_request_body(home, sequence, peer_host))

    intervals = [run_interval(submit, interval) for interval in range(INTERVALS)]
    return {"label": label, "peer_host": peer_host, "intervals": intervals, "passed": all(item["passed"] for item in intervals)}


def write_evidence(directory: Path, evidence: dict[str, Any]) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"admission-capacity-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}.json"
    path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def write_feature_report(evidence: dict[str, Any]) -> Path:
    """Feed capacity results into the same per-host XHTML/HTML smoke report."""
    from run_feature_smoke import write_report

    host = os.uname().nodename if hasattr(os, "uname") else os.environ.get("COMPUTERNAME", "local")
    doctor = evidence.get("doctor")
    healthy = isinstance(doctor, dict) and doctor.get("summary", {}).get("status") == "healthy"
    cases: list[dict[str, Any]] = [
        {
            "name": "doctor",
            "status": "PASS" if healthy else "FAIL",
            "detail": "ATM doctor healthy" if healthy else "capacity daemon doctor was not healthy",
            "origin": host,
            "destination": host,
        }
    ]
    for run in evidence.get("runs", []):
        intervals = run.get("intervals", [])
        passed = bool(run.get("passed"))
        accepted = sum(int(interval.get("accepted_count", 0)) for interval in intervals)
        total = len(intervals) * ADMISSIONS_PER_INTERVAL
        first_failure = next((interval.get("first_failure") for interval in intervals if interval.get("first_failure")), None)
        cases.append(
            {
                "name": f"admission capacity — {run.get('label', 'unknown')}",
                "status": "PASS" if passed else "FAIL",
                "detail": f"{accepted}/{total} accepted" + (f"; {first_failure}" if first_failure else ""),
                "origin": host,
                "destination": host,
            }
        )
    return write_report("admission-capacity", cases)


def run_capacity(atm_home: Path, evidence_directory: Path, accepting_host: str, unavailable_host: str) -> tuple[int, Path]:
    """Start one branch daemon, exercise public UDS API, retain evidence, then clean up."""
    require_isolated_os_user()
    home = validate_capacity_home(atm_home)
    require_clean_host_daemon_state(smoke_label="admission-capacity smoke")
    before = count_atm_daemon_processes()
    try:
        atm = release_binary(ROOT, "atm")
        daemon = release_binary(ROOT, "atm-daemon")
    except RuntimeError as error:
        raise SmokeError(str(error)) from error
    home.mkdir(parents=True, exist_ok=False)
    env = runtime_environment(home)
    process: subprocess.Popen[str] | None = None
    evidence: dict[str, Any] = {
        "release": {"atm": str(atm), "atm_daemon": str(daemon)},
        "atm_home": str(home),
        "runs": [],
        "stages": {
            "endpoint_discovery_ms": None,
            "public_response_ms": "recorded per interval as response_timing_ms; includes daemon validation, SQLite commit, and response write",
            "post_commit_signal": "not awaited; the public admission response is the boundary",
        },
    }
    try:
        prepare_capacity_roster(atm, env, home)
        process = subprocess.Popen([str(daemon)], cwd=home, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        await_daemon_ready(process)
        doctor = command_result([str(atm), "doctor", "--json"], timeout=10.0, env=env)
        if doctor["exit_code"] != 0:
            raise SmokeError(f"capacity doctor failed: {doctor['stderr'].strip()}")
        evidence["daemon_pid"] = process.pid
        evidence["doctor"] = json.loads(doctor["stdout"])
        configure_controlled_peer(atm, env, accepting_host, "capacity-accepting-peer")
        configure_controlled_peer(atm, env, unavailable_host, "capacity-unavailable-peer")
        endpoint_started = time.perf_counter()
        endpoint = local_endpoint()
        evidence["stages"]["endpoint_discovery_ms"] = (time.perf_counter() - endpoint_started) * 1_000
        if endpoint.kind == "uds" and not Path(str(endpoint.address)).exists():
            raise SmokeError(f"daemon did not publish public local socket {endpoint.address}")
        evidence["runs"] = [
            run_peer_case(endpoint, home, accepting_host, "accepting-configured-peer"),
            run_peer_case(endpoint, home, unavailable_host, "unavailable-configured-peer"),
        ]
        evidence["passed"] = all(run["passed"] for run in evidence["runs"])
    except (OSError, RuntimeError, ValueError, SmokeError) as error:
        evidence["passed"] = False
        evidence["failure"] = str(error)
    finally:
        if process is not None:
            terminate_process(process.pid)
            try:
                wait_for_process_exit(process.pid, process_label="admission-capacity daemon")
            except RuntimeError as error:
                evidence["passed"] = False
                evidence["cleanup_failure"] = str(error)
        try:
            assert_no_process_leak(before, count_atm_daemon_processes(), smoke_label="admission-capacity smoke")
        except RuntimeError as error:
            evidence["passed"] = False
            evidence["cleanup_failure"] = str(error)
        try:
            evidence["html_report"] = str(write_feature_report(evidence))
        except (OSError, RuntimeError, ValueError, SmokeError) as error:
            evidence["passed"] = False
            evidence["report_failure"] = str(error)
        evidence_path = write_evidence(evidence_directory, evidence)
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
    parser.add_argument("--evidence-dir", type=Path, default=ROOT / "artifacts" / "smoke" / "admission-capacity")
    parser.add_argument("--accepting-host", default="127.0.0.1")
    parser.add_argument("--unavailable-host", default="192.0.2.1")
    args = parser.parse_args()
    if args.atm_home is None:
        with tempfile.TemporaryDirectory(prefix="atm-capacity-parent-") as temp:
            home = Path(temp) / f"{CAPACITY_ROOT_PREFIX}home"
            code, evidence = run_capacity(home, args.evidence_dir, args.accepting_host, args.unavailable_host)
    else:
        code, evidence = run_capacity(args.atm_home, args.evidence_dir, args.accepting_host, args.unavailable_host)
    print(f"{'PASS' if code == 0 else 'FAIL'} admission-capacity evidence: {evidence}")
    return code


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"capacity smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

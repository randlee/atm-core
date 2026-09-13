"""Explicit transport and daemon setup helpers for the capacity smoke runner.

The runner owns orchestration; this module owns the concrete disposable
daemon, request, and socket operations.  Dependencies are imported directly so
test seams patch their defining module rather than mutating another module's
globals.
"""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
from queue import Empty
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
from typing import Any, Callable
import uuid

from scripts.smoke.admission_capacity_support import (
    ADMISSIONS_PER_INTERVAL,
    CAPACITY_DIRECT_PEER_PORT_ENV,
    CROCKFORD_BASE32,
    DAEMON_SWITCH,
    DAEMON_OUTPUT_TAIL_LINES,
    DEFAULT_CAPACITY_ROSTER,
    DESCRIPTOR_RESERVE,
    DIAGNOSTIC_DURATION_SECONDS,
    DIAGNOSTIC_SAMPLE_COUNT,
    MAX_IN_FLIGHT_REQUESTS,
    MANAGED_DAEMON_TIMEOUT_SECONDS,
    PEER_WIRE_SECURITY_MODES,
    READY_TIMEOUT_SECONDS,
    ROOT,
    TARGET_PROFILE_DURATION_SECONDS,
    AdmissionResult,
    CapacityRoster,
    DaemonOutputCapture,
    DisposableMtlsIdentity,
    HttpRequest,
    LocalEndpoint,
    distribution,
    os_account_home,
    require_capacity_benchmark_account,
    reap_owned_daemon,
)
from scripts.smoke.admission_capacity_support import (
    SmokeError,
    command_result,
    require_clean_host_daemon_state,
)

try:
    import resource
except ImportError:  # Windows has no POSIX rlimit API.
    resource = None

def benchmark_doctor_payload(result: dict[str, object]) -> dict[str, object]:
    """Validate the benchmark daemon's ready state from its public doctor response."""
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


def await_daemon_ready(process: subprocess.Popen[str], output: Any) -> None:
    """Wait only for the daemon's explicit readiness signal."""
    deadline = __import__("time").monotonic() + READY_TIMEOUT_SECONDS
    last_line = ""
    while __import__("time").monotonic() < deadline:
        try:
            line = output.ready_lines.get(timeout=min(0.1, max(0.0, deadline - __import__("time").monotonic())))
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
    daemon: Path, home: Path, env: dict[str, str], peer_wire_security: str = "mutual-tls",
) -> tuple[subprocess.Popen[str], Any]:
    """Start the shipped daemon with its ordinary explicit peer-wire mode."""
    if peer_wire_security not in PEER_WIRE_SECURITY_MODES:
        raise SmokeError("capacity peer-wire security must be `mutual-tls` or `plaintext-test`")
    require_clean_host_daemon_state(smoke_label="admission-capacity daemon launch")
    command = [str(daemon), "--peer-wire-security", peer_wire_security]
    if direct_peer_port := env.get(CAPACITY_DIRECT_PEER_PORT_ENV):
        command.extend(("--direct-peer-port", direct_peer_port))
    process = subprocess.Popen(command, cwd=home, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    output = DaemonOutputCapture.start(process)
    try:
        await_daemon_ready(process, output)
    except (OSError, SmokeError) as error:
        reap_owned_daemon(process)
        output.join()
        tails = output.evidence()
        stdout_tail = " | ".join(str(line) for line in tails.get("stdout_tail", [])[-8:]) or "<unavailable>"
        stderr_tail = " | ".join(str(line) for line in tails.get("stderr_tail", [])[-8:]) or "<unavailable>"
        raise SmokeError(f"{error}; daemon stdout tail: {stdout_tail}; daemon stderr tail: {stderr_tail}") from error
    return process, output


def prepare_capacity_roster(
    atm: Path, env: dict[str, str], home: Path, roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> None:
    """Create the disposable roster used exclusively by public benchmark writes."""
    for team, member, agent_type in ((roster.team, roster.agent, "lead"), (roster.team, roster.recipient, "general-purpose")):
        result = command_result(
            [str(atm), "teams", "add-member", team, member, "--agent-type", agent_type, "--home-dir", str(home), "--json"],
            timeout=15.0, env=env,
        )
        if result["exit_code"] != 0:
            raise SmokeError(f"could not create capacity roster member {member}: {result['stderr'].strip()}")


def http_request_body(
    home: Path, sequence: int, roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER, *, peer_origin: bool = False,
) -> bytes:
    """Build the documented /v1/atm/messages request; no dispatcher shortcut."""
    payload = {
        "home_dir": str(home), "current_dir": str(home), "caller_identity": roster.agent,
        "caller_team": roster.team, "to": {"agent": roster.recipient, "team": roster.team},
        "message_source": {"Inline": f"capacity-{sequence}"}, "summary_override": None,
        "requires_ack": False, "task_id": None, "parent_message_id": None,
        "thread_mode": None, "expires_at": None, "dry_run": False,
    }
    if peer_origin:
        payload.update(benchmark_origin_metadata(sequence))
    return json.dumps(payload, separators=(",", ":")).encode("utf-8")


def benchmark_origin_metadata(sequence: int) -> dict[str, str]:
    """Return the immutable provenance pair required by direct-peer ingress."""
    milliseconds = int(__import__("time").time() * 1_000)
    entropy = (uuid.uuid4().int ^ sequence) & ((1 << 80) - 1)
    value = (milliseconds << 80) | entropy
    encoded = "".join(CROCKFORD_BASE32[(value >> (5 * offset)) & 0x1F] for offset in range(25, -1, -1))
    return {"origin_message_id": encoded, "origin_timestamp": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")}


def cached_roster_heartbeat_body(sequence: int, roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER) -> bytes:
    """Build a heartbeat that validates against the warmed roster snapshot."""
    return json.dumps({
        "team": roster.team, "member": roster.agent, "pid": 90_000 + sequence,
        "observed_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "activity": "active_tool_use",
    }, separators=(",", ":")).encode("utf-8")


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
        raise SmokeError("capacity peer-wire security must be `mutual-tls` or `plaintext-test`")
    return value


def resolve_benchmark_target(target: str | None, transport: str | None) -> tuple[str, str | None, str | None]:
    """Resolve public targets without inventing a benchmark-only transport."""
    targets = {
        "sqlite": ("sqlite", None), "uds": ("uds", "mutual-tls"),
        "tcp": ("tcp", "plaintext-test"), "tcp-tls": ("tcp", "mutual-tls"),
    }
    if target is not None:
        selected_transport, peer_wire_security = targets[target]
        if transport is not None and transport != selected_transport:
            raise SmokeError(f"benchmark target {target!r} requires transport {selected_transport!r}")
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


def direct_peer_endpoint(port: int, identity: DisposableMtlsIdentity | None = None) -> LocalEndpoint:
    """Return the fixed direct-peer listener rather than the local HTTP record."""
    return LocalEndpoint(
        "tcp", ("127.0.0.1", port), direct_peer=True,
        tls_server_name=identity.host if identity is not None else None,
        tls_certificate_bundle=identity.certificate_bundle if identity is not None else None,
    )


def disposable_peer_host(roster: CapacityRoster) -> str:
    """Resolve one durable authority for a local disposable mTLS peer."""
    configured = os.environ.get("ATM_CAPACITY_PEER_HOST")
    host = (configured if configured is not None else socket.getfqdn()).strip().rstrip(".")
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
        raise SmokeError("benchmark mTLS authority must be a durable DNS or mDNS hostname; set ATM_CAPACITY_PEER_HOST explicitly when local DNS is incomplete")
    return authority


def _required_command(argv: list[str], env: dict[str, str], description: str) -> None:
    result = command_result(argv, timeout=20.0, env=env)
    if result["exit_code"] == 0:
        return
    detail = result["stderr"].strip() or result["stdout"].strip() or "no diagnostic output"
    raise SmokeError(f"could not {description}: {detail}")


def provision_disposable_mtls_identity(
    atm: Path, env: dict[str, str], home: Path, roster: CapacityRoster, direct_peer_port: int,
) -> DisposableMtlsIdentity:
    """Install a self-contained, short-lived peer configuration before launch."""
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
        "[req]\nprompt = no\ndistinguished_name = subject\nx509_extensions = extensions\n"
        "[subject]\n" f"CN = {host}\n" "[extensions]\n" f"subjectAltName = DNS:{host}\n"
        "basicConstraints = critical,CA:TRUE\nkeyUsage = critical,digitalSignature,keyEncipherment,keyCertSign\n"
        "extendedKeyUsage = serverAuth,clientAuth\n", encoding="utf-8",
    )
    _required_command([
        openssl, "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:prime256v1",
        "-pkeyopt", "ec_param_enc:named_curve", "-out", str(private_key),
    ], env, "generate disposable P-256 mTLS private key")
    _required_command([
        openssl, "req", "-x509", "-new", "-key", str(private_key), "-out", str(certificate),
        "-days", "2", "-config", str(config),
    ], env, "generate disposable P-256 mTLS certificate")
    bundle.write_bytes(certificate.read_bytes() + private_key.read_bytes())
    try:
        der = ssl.PEM_cert_to_DER_cert(certificate.read_text(encoding="ascii"))
    except (OSError, ValueError) as error:
        raise SmokeError(f"could not calculate disposable mTLS certificate fingerprint: {error}") from error
    fingerprint = hashlib.sha256(der).hexdigest()
    _required_command([
        str(atm), "peer", "interface", "set", "--bind", f"127.0.0.1:{direct_peer_port}",
        "--advertise-host", host, "--enabled",
    ], env, "save disposable mTLS interface")
    _required_command([
        str(atm), "peer", "certificate", "init", "--fingerprint", fingerprint,
        "--private-key-ref", str(bundle), "--yes",
    ], env, "save disposable mTLS local identity")
    _required_command([
        str(atm), "peer", "trust", "add", "--host", host, "--fingerprint", fingerprint,
        "--https-port", str(direct_peer_port), "--yes",
    ], env, "save disposable mTLS trusted peer")
    return DisposableMtlsIdentity(host, bundle, fingerprint)


def read_http_response(stream: socket.socket, buffered: bytearray | None = None) -> tuple[int, int, str | None]:
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
    return status, frame_end, body[:512].decode("utf-8", "replace") if status >= 400 else None


def tls_client_context(endpoint: LocalEndpoint) -> ssl.SSLContext | None:
    """Prepare one immutable mTLS client context for a benchmark profile."""
    if endpoint.tls_server_name is None:
        return None
    if endpoint.tls_certificate_bundle is None:
        raise SmokeError("direct mTLS endpoint omitted its benchmark identity bundle")
    context = ssl.create_default_context(ssl.Purpose.SERVER_AUTH, cafile=str(endpoint.tls_certificate_bundle))
    context.load_cert_chain(str(endpoint.tls_certificate_bundle))
    return context


def submit_connection(
    endpoint: LocalEndpoint, requests: list[HttpRequest], tls_context: ssl.SSLContext | None = None,
) -> list[AdmissionResult]:
    """Submit consecutive real requests over one selected public connection."""
    started = time.perf_counter()
    capability = f"X-ATM-Local-Capability: {endpoint.capability}\r\n".encode("ascii") if endpoint.capability is not None else b""
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
                stream = context.wrap_socket(raw_stream, server_hostname=endpoint.tls_server_name)
            else:
                stream = raw_stream
            frames: list[bytes] = []
            authority = (
                f"Host: {endpoint.tls_server_name or str(endpoint.address[0])}:{endpoint.address[1]}\r\n".encode("ascii")
                if endpoint.kind == "tcp" else b"Host: localhost\r\n"
            )
            for index, request in enumerate(requests):
                connection = "close" if index + 1 == len(requests) else "keep-alive"
                frames.append(
                    f"POST {request.path} HTTP/1.1\r\n".encode("ascii") + b"Content-Type: application/json\r\n"
                    + authority + capability + f"Content-Length: {len(request.body)}\r\nConnection: {connection}\r\n\r\n".encode("ascii") + request.body
                )
            response_buffer = bytearray()
            for start in range(0, len(frames), MAX_IN_FLIGHT_REQUESTS):
                batch = frames[start:start + MAX_IN_FLIGHT_REQUESTS]
                request_started = time.perf_counter()
                stream.sendall(b"".join(batch))
                for request, frame in zip(requests[start:start + MAX_IN_FLIGHT_REQUESTS], batch):
                    status, response_bytes, response_summary = read_http_response(stream, response_buffer)
                    results.append(AdmissionResult(
                        status=status, elapsed_ms=(time.perf_counter() - request_started) * 1_000,
                        failure=None if status == request.expected_status else f"HTTP {status}: {response_summary or 'no response body'}",
                        request_bytes=len(frame), response_bytes=response_bytes, response_summary=response_summary,
                    ))
            if stream is not raw_stream:
                stream.close()
        return results
    except (OSError, SmokeError) as error:
        return results + [AdmissionResult(status=0, elapsed_ms=(time.perf_counter() - started) * 1_000, failure=str(error))]


def admission_connection_worker_limit(requested_workers: int) -> int:
    """Bound concurrent client sockets by the process soft descriptor limit."""
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
    submit: Callable[[int, int], list[AdmissionResult]], interval: int, frames_per_connection: int,
    workers: int, requested_messages: int = ADMISSIONS_PER_INTERVAL, expected_status: int = 201,
) -> dict[str, Any]:
    """Run one exactly-sized admission interval without retrying failed writes."""
    if requested_messages <= 0:
        raise SmokeError("requested messages must be positive")
    started = time.perf_counter()
    results: list[AdmissionResult] = []
    connections = (requested_messages + frames_per_connection - 1) // frames_per_connection
    connection_workers = min(connections, admission_connection_worker_limit(workers))
    with ThreadPoolExecutor(max_workers=connection_workers, thread_name_prefix="atm-capacity") as executor:
        futures = [executor.submit(submit, interval * requested_messages + sequence * frames_per_connection, min(frames_per_connection, requested_messages - sequence * frames_per_connection)) for sequence in range(connections)]
        for future in as_completed(futures):
            results.extend(future.result())
    elapsed_seconds = time.perf_counter() - started
    accepted = sum(result.status == expected_status for result in results)
    failures = [result.failure or f"HTTP {result.status}" for result in results if result.status != expected_status]
    latencies = [result.elapsed_ms for result in results]
    latency_distribution = distribution(latencies) if latencies else {"min": 0.0, "p50": 0.0, "p95": 0.0, "p99": 0.0, "max": 0.0}
    error_free = accepted == requested_messages and not failures
    request_bytes = sum(result.request_bytes for result in results)
    response_bytes = sum(result.response_bytes for result in results)
    return {
        "interval": interval + 1, "accepted_count": accepted, "response_count": len(results),
        "elapsed_seconds": elapsed_seconds, "admissions_per_second": accepted / elapsed_seconds if elapsed_seconds else 0.0,
        "latency_ms": latency_distribution, "connections": connections, "connection_workers": connection_workers,
        "request_frames_per_second": len(results) / elapsed_seconds if elapsed_seconds else 0.0,
        "connections_per_second": connections / elapsed_seconds if elapsed_seconds else 0.0,
        "requested_count": requested_messages, "time_to_send_1k_s": elapsed_seconds * (1_000 / max(accepted, 1)),
        "application_wire_bytes": {"request": request_bytes, "response": response_bytes, "total": request_bytes + response_bytes},
        "application_wire_bytes_per_second": (request_bytes + response_bytes) / elapsed_seconds if elapsed_seconds else 0.0,
        "error_free": error_free, "bytes_per_second": (request_bytes + response_bytes) / elapsed_seconds if elapsed_seconds else 0.0,
        "first_failure": failures[0] if failures else None, "passed": error_free,
    }

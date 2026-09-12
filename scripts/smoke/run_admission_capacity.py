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


from scripts.smoke import admission_capacity_support as _admission_support

globals().update({name: getattr(_admission_support, name) for name in _admission_support.__all__})
_admission_support._PUBLIC_NAMESPACE = globals()
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
    roster: CapacityRoster = DEFAULT_CAPACITY_ROSTER,
) -> dict[str, Any]:
    """Collect at least ten independent intervals over one sustained profile.

    The prepared request bytes are deliberately outside each interval's timer.
    A capacity result measures the released daemon's admission path, not Python
    UUID/timestamp creation, JSON serialization, or worker-thread/GIL work.
    This preserves the exact request payload, including direct-peer provenance.
    """
    if sample_count <= 0:
        raise SmokeError("capacity sample count must be positive")
    if target_duration_seconds <= 0:
        raise SmokeError("capacity target duration must be positive")
    profile_tls_context = tls_client_context(endpoint)

    def interval_requests(interval: int) -> list[HttpRequest]:
        """Create one unique, immutable request batch before timing begins."""
        sequence_start = interval * requested_messages
        if operation == "write":
            return [
                HttpRequest(
                    "/v1/atm/messages",
                    http_request_body(
                        home,
                        sequence_start + offset,
                        roster,
                        peer_origin=endpoint.direct_peer,
                    ),
                    201,
                )
                for offset in range(requested_messages)
            ]
        if operation == "cached_roster_heartbeat":
            return [
                HttpRequest(
                    "/v1/atm/heartbeat",
                    cached_roster_heartbeat_body(sequence_start + offset, roster),
                    200,
                )
                for offset in range(requested_messages)
            ]
        raise SmokeError(f"unsupported capacity benchmark operation {operation!r}")

    intervals: list[dict[str, Any]] = []
    elapsed_seconds = 0.0
    while len(intervals) < sample_count or elapsed_seconds < target_duration_seconds:
        interval_index = len(intervals)
        requests = interval_requests(interval_index)

        def submit(sequence: int, message_count: int) -> list[AdmissionResult]:
            offset = sequence - interval_index * requested_messages
            selected = requests[offset:offset + message_count]
            if offset < 0 or len(selected) != message_count:
                raise SmokeError("timed interval selected requests outside its prepared batch")
            return submit_connection(endpoint, selected, profile_tls_context)

        interval = run_interval(
            submit,
            interval_index,
            frames_per_connection,
            workers,
            requested_messages,
            expected_status=expected_status,
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


def run_direct_sqlite_writer_profile(
    benchmark_binary: Path,
    environment: dict[str, str],
    roster: CapacityRoster,
    requested_messages: int,
    sample_count: int,
    workers: int,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Measure the timed Tokio durable-admission writer without raw SQL or HTTP.

    The benchmark-owned daemon first creates the disposable roster and is then
    stopped.  The direct binary calls ``save_message_if_absent_async`` so this
    lane includes the bounded writer queue, one-millisecond batch transaction,
    and durable commit.  It excludes the public socket/codec measured by UDS
    and TCP, and the canonical preparation path retained as a diagnostic.
    """
    direct_environment = dict(environment)
    direct_environment.update(
        {
            "ATM_CAPACITY_STORAGE_TEAM": roster.team,
            "ATM_CAPACITY_STORAGE_SENDER": roster.agent,
            "ATM_CAPACITY_STORAGE_RECIPIENT": roster.recipient,
        }
    )
    result = subprocess.run(
        [
            str(benchmark_binary), "--direct-storage-admission", str(requested_messages),
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
        raise SmokeError(f"direct sqlite-writer benchmark failed: {detail}")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SmokeError("direct sqlite-writer benchmark returned malformed JSON") from error
    if not isinstance(payload, dict) or payload.get("kind") != "async_storage_admission":
        raise SmokeError("direct sqlite-writer benchmark returned the wrong measurement kind")
    direct_intervals = payload.get("intervals")
    if not isinstance(direct_intervals, list) or len(direct_intervals) < sample_count:
        raise SmokeError("direct sqlite-writer benchmark returned too few intervals")
    intervals: list[dict[str, Any]] = []
    for index, item in enumerate(direct_intervals, start=1):
        if not isinstance(item, dict):
            raise SmokeError("direct sqlite-writer interval is malformed")
        accepted = int(item.get("accepted_count", -1))
        requested = int(item.get("requested_count", -1))
        elapsed = float(item.get("elapsed_seconds", 0.0))
        rate = float(item.get("admissions_per_second", 0.0))
        if requested != requested_messages or accepted < 0 or accepted > requested or elapsed <= 0 or rate < 0:
            raise SmokeError("direct sqlite-writer interval has invalid counts or timing")
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
    direct_writer = sqlite_writer_probe() if transport == "sqlite" else None
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
            # This legacy v3 schema label denotes the direct benchmark writer;
            # the measurement kind below makes the async-storage seam explicit.
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
            "sqlite_transaction": (
                "measured by the SQLite target's direct durable-admission completion "
                "and by public target response latency"
            ),
            "post_commit_received_hook": "active in the shipped daemon; any hook failure is returned as a successful write warning",
            "response_write": "included only in public UDS/TCP/TCP+TLS admission response latency",
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
        checkpoint_closed_database(benchmark_account.durable_state_root / "mail.db")
        if daemon_output is not None:
            daemon_output.join()
            if output_key is not None:
                evidence[output_key] = daemon_output.evidence()
        process = None
        daemon_output = None

    def start_daemon(mode: str = launch_peer_wire_security) -> None:
        """Start the exact released daemon and retain its ownership handle."""
        nonlocal process, daemon_output
        process, daemon_output = start_capacity_daemon(daemon, home, env, mode)

    def require_healthy_doctor() -> None:
        """Require public doctor health after the disposable roster is present."""
        if process is None:
            raise SmokeError("capacity doctor requested before the benchmark daemon started")
        doctor = command_result(
            [str(atm), "doctor", "--json"],
            timeout=10.0,
            env=benchmark_runtime_client_environment(env),
        )
        evidence["doctor"] = benchmark_doctor_payload(doctor)
        evidence["doctor_status"] = "passed"
        evidence["daemon_pid"] = process.pid

    def start_and_doctor(mode: str = launch_peer_wire_security) -> None:
        """Start the daemon and validate its already-provisioned public state."""
        start_daemon(mode)
        require_healthy_doctor()

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
            run_lifecycle_phase(evidence, "snapshot", lambda: start_daemon("plaintext-test"))
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
        run_lifecycle_phase(evidence, "snapshot", start_daemon)
        run_lifecycle_phase(
            evidence, "stop", lambda: stop_owned_daemon(output_key="pre_snapshot_daemon_output"),
        )
        snapshot = run_lifecycle_phase(evidence, "snapshot", create_verified_snapshot)
        evidence["clean_baseline_snapshot"] = snapshot_evidence(snapshot)
        evidence["clean_baseline_snapshot"]["sidecars_absent"] = True

        # Roster creation belongs strictly after clean-baseline publication.
        run_lifecycle_phase(evidence, "profile", start_daemon)
        run_lifecycle_phase(
            evidence, "profile", lambda: prepare_capacity_roster(atm, env, home, roster),
        )
        run_lifecycle_phase(evidence, "profile", require_healthy_doctor)
        if transport == "sqlite":
            if direct_writer is None:
                raise SmokeError("direct production-writer binary is unavailable")
            run_lifecycle_phase(evidence, "stop", stop_owned_daemon)
            profile, direct_measurement = run_lifecycle_phase(
                evidence,
                "profile",
                lambda: run_direct_sqlite_writer_profile(
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

        # This factual completion marker is distinct from acceptance.  The
        # canonical classifier applies the reviewed floor at v4 emission.
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
        (
            0 if published_result is not None and published_result.status == "PASS" else 1
        ) if v4_emission is not None else (0 if evidence.get("passed") else 1),
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
    baselines = load_baselines(BASELINES_PATH)
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
        if run.result is None:
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
    campaign_status = classify_status(
        required_targets=required_targets(os_name),
        observed_targets=tuple(result.target for result in published_results),
        target_statuses=tuple(result.status for result in published_results),
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
    return 0 if campaign.status == "PASS" else 1


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

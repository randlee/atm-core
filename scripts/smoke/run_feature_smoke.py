#!/usr/bin/env python3
"""Internal implementation of the canonical ``just smoke`` feature command.

Run one progressively stronger smoke feature against the selected daemon.

The runner never starts, stops, switches, or configures a daemon.  Use the
daemon-switch skill before invoking ``just smoke``.  Local identity comes from the normal
CLI environment: ``ATM_IDENTITY`` and ``ATM_TEAM``.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
from html import escape
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import shlex
import socket
import subprocess
import sys
import tempfile
import time
from typing import Any

from feature_smoke_report import (
    render_cross_host_section,
    render_feature_pane,
    render_host_header,
    summarize_cases,
)
from run_inbound_peer_smoke import PANE_TEMPLATE, compose
from smoke_common import (
    SmokeError,
    advertised_host_from_value as advertised_host_from_json,
    command_result as command,
    message_id_from_value as message_id,
)


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_FEATURES = frozenset({"fast", "normal", "thorough"})
LOCALHOST = "localhost"
LOCAL_IP = "local-ip"
LOCAL_IP_ALIAS = "local-up"
CROSSHOST = "crosshost"
PEER_PREFLIGHT = "peer-preflight"
CROSSHOST_SEND = "crosshost-send"
CROSSHOST_ACK = "crosshost-ack"
CROSSHOST_CURL_PLAINTEXT = "crosshost-curl-plain"
CROSSHOST_CURL_MTLS = "crosshost-curl-tls"
ADMISSION_CAPACITY = "admission-capacity"
DOCTOR_BODY = '{"home_dir":"","current_dir":"","team_override":null,"caller_team":null,"caller_identity":null}'
DEFAULT_LIVE_REPETITIONS = 10


def require_environment() -> tuple[str, str, str]:
    identity = os.environ.get("ATM_IDENTITY", "").strip()
    team = os.environ.get("ATM_TEAM", "").strip()
    if not identity or not team:
        raise SmokeError("set ATM_IDENTITY and ATM_TEAM before running live daemon smoke")
    return os.environ.get("ATM_SMOKE_ATM", "atm"), identity, team


def parse_json(result: dict[str, Any], label: str) -> Any:
    if result["exit_code"] != 0:
        raise SmokeError(f"{label} failed: {result['stderr'].strip() or result['stdout'].strip()}")
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise SmokeError(f"{label} did not return JSON: {error}") from error


def branch_version() -> str:
    """Return the checked-out ATM release version, without trusting PATH."""
    metadata = parse_json(
        command(["cargo", "metadata", "--no-deps", "--format-version", "1"]),
        "cargo metadata",
    )
    packages = metadata.get("packages", []) if isinstance(metadata, dict) else []
    selected = {
        package.get("name"): package.get("version")
        for package in packages
        if package.get("name") in {"agent-team-mail", "atm-daemon"}
    }
    versions = set(selected.values())
    if set(selected) != {"agent-team-mail", "atm-daemon"} or len(versions) != 1 or not isinstance(next(iter(versions)), str):
        raise SmokeError("cargo metadata did not expose one shared atm/atm-daemon version")
    return next(iter(versions))


def selected_message(value: Any, expected: str) -> dict[str, Any] | None:
    if isinstance(value, dict):
        for key in ("message", "selected_message"):
            child = value.get(key)
            if isinstance(child, dict) and child.get("message_id", child.get("messageId")) == expected:
                return child
        messages = value.get("messages")
        if isinstance(messages, list):
            for child in messages:
                if isinstance(child, dict) and child.get("message_id", child.get("messageId")) == expected:
                    return child
    return None


def wait_for_message(atm: str, team: str, expected: str, timeout: float = 12.0) -> dict[str, Any] | None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = command([atm, "read", "--team", team, "--message-id", expected, "--json"])
        if result["exit_code"] == 0:
            try:
                found = selected_message(json.loads(result["stdout"]), expected)
            except json.JSONDecodeError:
                found = None
            if found is not None:
                return found
        time.sleep(0.4)
    return None


def reply_message_id(value: Any) -> str:
    """Read the reply ULID from the public ``atm ack --json`` contract."""
    if not isinstance(value, dict):
        raise SmokeError("acknowledgement did not return a JSON object")
    disposition = value.get("reply_disposition")
    if not isinstance(disposition, dict) or disposition.get("kind") != "sent":
        raise SmokeError("acknowledgement did not report a sent reply")
    reply_id = disposition.get("reply_message_id")
    if not isinstance(reply_id, str) or not reply_id:
        raise SmokeError("acknowledgement response omitted reply_message_id")
    return reply_id


def message_has_text(message: dict[str, Any] | None, expected: str) -> bool:
    return message is not None and message.get("text") == expected


def advertised_host(atm: str) -> str:
    override = os.environ.get("ATM_SMOKE_ADVERTISED_HOST", "").strip()
    if override:
        return override
    interfaces = parse_json(command([atm, "peer", "interface", "list", "--json"]), "peer interface list")
    return advertised_host_from_json(interfaces)


def local_advertised_ipv4(host: str) -> str:
    """Resolve the enabled peer authority to a usable same-host IPv4 address.

    The local-IP lane must use an address that the daemon has explicitly
    advertised and the CLI can canonicalize through trusted-peer records. A
    default-route UDP probe can instead select a VPN/tunnel address unrelated
    to that authority, producing a false smoke failure.
    """
    try:
        candidates = [record[4][0] for record in socket.getaddrinfo(host, None, socket.AF_INET)]
    except OSError as error:
        raise SmokeError(f"enabled advertised host '{host}' could not be resolved: {error}") from error

    for candidate in candidates:
        address = ipaddress.ip_address(candidate)
        if isinstance(address, ipaddress.IPv4Address) and not (
            address.is_loopback or address.is_unspecified or address.is_multicast or address.is_link_local
        ):
            return str(address)
    raise SmokeError(f"enabled advertised host '{host}' has no usable non-loopback IPv4 address for local-IP smoke")


def doctor_ready(report: Any, expected_version: str) -> bool:
    """Return whether one public doctor response proves a matched ready pair."""
    return (
        isinstance(report, dict)
        and report.get("summary", {}).get("status") == "healthy"
        and report.get("runtime_status", {}).get("readiness") == "ready"
        and report.get("client_context", {}).get("version") == expected_version
        and report.get("daemon_context", {}).get("version") == expected_version
    )


def remote_command(peer: str, remote_atm: str, args: list[str], timeout: float = 20.0) -> dict[str, Any]:
    """Invoke only the public CLI on an already-running SSH peer."""
    remote_identity = os.environ.get("ATM_SMOKE_REMOTE_IDENTITY", "").strip()
    remote_team = os.environ.get("ATM_SMOKE_REMOTE_TEAM", "").strip()
    if remote_identity and remote_team:
        return command(
            ["ssh", peer, "env", f"ATM_IDENTITY={remote_identity}", f"ATM_TEAM={remote_team}", remote_atm, *args],
            timeout=timeout,
        )
    return command(["ssh", peer, remote_atm, *args], timeout=timeout)


def remote_context() -> tuple[str, str]:
    """Return the explicit recipient identity required for live peer sends."""
    identity = os.environ.get("ATM_SMOKE_REMOTE_IDENTITY", "").strip()
    team = os.environ.get("ATM_SMOKE_REMOTE_TEAM", "").strip()
    if not identity or not team:
        raise SmokeError(
            "set ATM_SMOKE_REMOTE_IDENTITY and ATM_SMOKE_REMOTE_TEAM for cross-host ATM delivery smoke"
        )
    return identity, team


def remote_shell(peer: str, script: str, timeout: float = 20.0) -> dict[str, Any]:
    """Run one bounded, quoted diagnostic command in the peer's shell."""
    return command(["ssh", peer, f"sh -lc {shlex.quote(script)}"], timeout=timeout)


@contextmanager
def remote_certificate_workspace(peer: str):
    """Yield a unique remote certificate workspace and remove only its files."""
    created = remote_shell(peer, "mktemp -d")
    if created["exit_code"] != 0:
        raise SmokeError(f"{peer} could not create a temporary certificate directory: {created['stderr'].strip()}")
    workspace = created["stdout"].strip()
    if not workspace:
        raise SmokeError(f"{peer} returned no temporary certificate directory")

    completed = False
    try:
        yield workspace
        completed = True
    finally:
        local_public = f"{workspace}/local-public.pem"
        peer_public = f"{workspace}/peer-public.pem"
        cleanup = remote_shell(
            peer,
            "rm -f "
            f"{shlex.quote(local_public)} {shlex.quote(peer_public)} "
            f"&& rmdir {shlex.quote(workspace)}",
        )
        if completed and cleanup["exit_code"] != 0:
            raise SmokeError(
                f"{peer} could not remove temporary certificate workspace: {cleanup['stderr'].strip()}"
            )


def certificate_bundle(atm: str) -> str:
    certificate = parse_json(command([atm, "peer", "certificate", "show", "--json"]), "peer certificate show")
    bundle = certificate.get("private_key_ref") if isinstance(certificate, dict) else None
    if not isinstance(bundle, str) or not bundle:
        raise SmokeError("peer certificate show did not expose private_key_ref")
    return bundle


def certificate_authority(pem: Path) -> str:
    result = command(["openssl", "x509", "-in", str(pem), "-noout", "-subject", "-nameopt", "RFC2253"])
    if result["exit_code"] != 0:
        raise SmokeError(f"could not inspect public certificate: {result['stderr'].strip()}")
    match = re.search(r"CN=([^,\n]+)", result["stdout"])
    if match is None:
        raise SmokeError("public certificate subject has no common name")
    return match.group(1)


def resolve_dns_addresses(host: str) -> list[str]:
    """Return every address the local resolver provides for a peer hostname."""
    try:
        records = socket.getaddrinfo(host, 43101, type=socket.SOCK_STREAM)
    except OSError as error:
        raise SmokeError(f"DNS resolution for {host} failed: {error}") from error
    return sorted({record[4][0] for record in records})


def remote_resolve_dns_addresses(peer: str, host: str) -> list[str]:
    """Resolve a hostname through the remote computer's normal resolver."""
    script = (
        "import json, socket; "
        f"print(json.dumps(sorted({{item[4][0] for item in socket.getaddrinfo({host!r}, 43101, type=socket.SOCK_STREAM)}})))"
    )
    result = remote_shell(peer, f"python3 -c {shlex.quote(script)}")
    try:
        addresses = parse_json(result, f"{peer} DNS resolution for {host}")
    except SmokeError:
        raise
    if not isinstance(addresses, list) or not all(isinstance(address, str) for address in addresses):
        raise SmokeError(f"{peer} DNS resolution for {host} did not return an address list")
    return addresses


def add_dns_case(
    cases: list[dict[str, Any]], name: str, origin: str, destination: str, hostname: str, expected_ip: str,
    resolver: Any,
) -> None:
    """Record real OS DNS resolution and require the daemon's advertised IP."""
    try:
        addresses = resolver(hostname)
        passed = expected_ip in addresses
        detail = f"{hostname} -> {', '.join(addresses)}"
        if not passed:
            detail += f"; missing advertised IP {expected_ip}"
        add_case(cases, name, passed, detail, origin=origin, destination=destination)
    except SmokeError as error:
        add_case(cases, name, False, str(error), origin=origin, destination=destination)


def mtls_rejected_before_http(result: dict[str, Any]) -> bool:
    """Return whether curl observed a TLS rejection with no HTTP response.

    The negative smoke trusts the server certificate but supplies no client
    certificate. A nonzero curl exit plus status 000 proves the connection
    stopped in the mTLS handshake, before Hyper can parse an HTTP request or
    the router can dispatch it.
    """
    return result["exit_code"] != 0 and result["stdout"].strip() == "000"


def add_mtls_rejection_case(
    cases: list[dict[str, Any]], name: str, result: dict[str, Any], origin: str, destination: str
) -> None:
    """Record a bounded, public pre-router mTLS negative result."""
    passed = mtls_rejected_before_http(result)
    if passed:
        detail = "mTLS rejected the unauthenticated client before any HTTP status"
    else:
        http_status = result["stdout"].strip() or "no status marker"
        detail = (
            "expected a pre-router mTLS rejection "
            f"(nonzero curl exit and HTTP status 000); got exit={result['exit_code']}, status={http_status}"
        )
    add_case(cases, name, passed, detail, origin=origin, destination=destination)


def curl_doctor(
    cases: list[dict[str, Any]], peer: str, atm: str, remote_atm: str, remote_host: str, expected_version: str,
    *, plaintext: bool,
) -> None:
    """Call the real peer doctor route with curl in both directions.

    Plaintext mode is only meaningful after the caller has started both
    managed smoke daemons with ``--peer-wire-security plaintext-test``. The
    runner never changes that daemon state itself.
    """
    try:
        local_bundle = certificate_bundle(atm)
        remote_certificate = parse_json(
            remote_command(peer, remote_atm, ["peer", "certificate", "show", "--json"]),
            f"{peer} peer certificate show",
        )
        remote_bundle = remote_certificate.get("private_key_ref") if isinstance(remote_certificate, dict) else None
        if not isinstance(remote_bundle, str) or not remote_bundle:
            raise SmokeError(f"{peer} peer certificate show did not expose private_key_ref")
        with tempfile.TemporaryDirectory(prefix="atm-smoke-certs-") as temp:
            tempdir = Path(temp)
            local_public = tempdir / "local-public.pem"
            remote_public = tempdir / "remote-public.pem"
            local_export = command(["openssl", "x509", "-in", local_bundle, "-out", str(local_public)])
            if local_export["exit_code"] != 0:
                raise SmokeError(f"could not export local public certificate: {local_export['stderr'].strip()}")
            with remote_certificate_workspace(peer) as remote_tempdir:
                remote_local_ca = f"{remote_tempdir}/local-public.pem"
                remote_public_path = f"{remote_tempdir}/peer-public.pem"
                export_remote = remote_shell(peer, f"openssl x509 -in {shlex.quote(remote_bundle)} -out {shlex.quote(remote_public_path)}")
                if export_remote["exit_code"] != 0:
                    raise SmokeError(f"{peer} could not export public certificate: {export_remote['stderr'].strip()}")
                copied_local = command(["scp", str(local_public), f"{peer}:{remote_local_ca}"])
                if copied_local["exit_code"] != 0:
                    raise SmokeError(f"could not copy local public certificate to {peer}: {copied_local['stderr'].strip()}")
                copied_remote = command(["scp", f"{peer}:{remote_public_path}", str(remote_public)])
                if copied_remote["exit_code"] != 0:
                    raise SmokeError(f"could not copy {peer} public certificate: {copied_remote['stderr'].strip()}")
                local_authority = certificate_authority(local_public)
                remote_authority = certificate_authority(remote_public)
                scheme = "http" if plaintext else "https"
                local_url = f"{scheme}://{local_authority}:43101/v1/atm/doctor"
                remote_url = f"{scheme}://{remote_authority}:43101/v1/atm/doctor"
                headers = ["-H", "Content-Type: application/json"]
                remote_curl = ["curl", "--silent", "--show-error", "--fail", "--connect-timeout", "2", "--max-time", "5", "-X", "GET", *headers]
                if not plaintext:
                    remote_curl.extend(["--cert", remote_bundle, "--cacert", remote_local_ca])
                remote_curl.extend(["--resolve", f"{local_authority}:43101:{advertised_host(atm)}", "--data", DOCTOR_BODY, local_url])
                remote_result = remote_shell(peer, " ".join(shlex.quote(value) for value in remote_curl))
                remote_report = parse_json(remote_result, f"{peer} curl doctor to local")
                add_case(cases, f"{peer} curl {'plaintext' if plaintext else 'mTLS'} to local doctor", doctor_ready(remote_report, expected_version), "HTTP 200 real daemon doctor" if doctor_ready(remote_report, expected_version) else "doctor response was not healthy/ready", origin=peer, destination=platform.node())
                local_curl = ["curl", "--silent", "--show-error", "--fail", "--connect-timeout", "2", "--max-time", "5", "-X", "GET", *headers]
                if not plaintext:
                    local_curl.extend(["--cert", local_bundle, "--cacert", str(remote_public)])
                local_curl.extend(["--resolve", f"{remote_authority}:43101:{remote_host}", "--data", DOCTOR_BODY, remote_url])
                local_result = command(local_curl)
                local_report = parse_json(local_result, f"curl doctor to {peer}")
                add_case(cases, f"local curl {'plaintext' if plaintext else 'mTLS'} to {peer} doctor", doctor_ready(local_report, expected_version), "HTTP 200 real daemon doctor" if doctor_ready(local_report, expected_version) else "doctor response was not healthy/ready", origin=platform.node(), destination=peer)
                if not plaintext:
                    # Deliberately omit --cert but retain the peer CA and the
                    # exact target address. --write-out emits 000 only when
                    # no HTTP response exists, so this checks the live mTLS
                    # admission gate rather than a router-level error.
                    remote_negative = [
                        "curl", "--silent", "--show-error", "--connect-timeout", "2", "--max-time", "5",
                        "--write-out", "%{http_code}", "-X", "GET", *headers, "--cacert", remote_local_ca,
                        "--resolve", f"{local_authority}:43101:{advertised_host(atm)}", "--data", DOCTOR_BODY, local_url,
                    ]
                    remote_negative_result = remote_shell(
                        peer, " ".join(shlex.quote(value) for value in remote_negative)
                    )
                    add_mtls_rejection_case(
                        cases,
                        f"local rejects unauthenticated mTLS client from {peer} before HTTP",
                        remote_negative_result,
                        peer,
                        platform.node(),
                    )
                    local_negative = [
                        "curl", "--silent", "--show-error", "--connect-timeout", "2", "--max-time", "5",
                        "--write-out", "%{http_code}", "-X", "GET", *headers, "--cacert", str(remote_public),
                        "--resolve", f"{remote_authority}:43101:{remote_host}", "--data", DOCTOR_BODY, remote_url,
                    ]
                    local_negative_result = command(local_negative)
                    add_mtls_rejection_case(
                        cases,
                        f"{peer} rejects unauthenticated mTLS client from local before HTTP",
                        local_negative_result,
                        platform.node(),
                        peer,
                    )
            # These checks use each host's ordinary DNS resolver. The mTLS
            # request below intentionally omits --resolve, proving that the
            # TCP connection follows DNS rather than the explicit-IP proof.
            add_dns_case(
                cases,
                f"local DNS resolves {peer} peer",
                platform.node(),
                peer,
                remote_authority,
                remote_host,
                resolve_dns_addresses,
            )
            local_hostname = platform.node()
            local_advertised_ip = advertised_host(atm)
            add_dns_case(
                cases,
                f"{peer} DNS resolves local peer",
                peer,
                platform.node(),
                local_hostname,
                local_advertised_ip,
                lambda hostname: remote_resolve_dns_addresses(peer, hostname),
            )
            dns_curl = [
                "curl", "--silent", "--show-error", "--fail", "--connect-timeout", "2", "--max-time", "5", "-X", "GET",
                *headers,
            ]
            if not plaintext:
                dns_curl.extend(["--cert", local_bundle, "--cacert", str(remote_public)])
            dns_curl.extend(["--data", DOCTOR_BODY, remote_url])
            dns_report = parse_json(command(dns_curl), f"DNS curl doctor to {peer}")
            add_case(
                cases,
                f"local DNS curl {'plaintext' if plaintext else 'mTLS'} to {peer} doctor",
                doctor_ready(dns_report, expected_version),
                "HTTP 200 real daemon doctor via DNS" if doctor_ready(dns_report, expected_version) else "doctor response was not healthy/ready",
                origin=platform.node(),
                destination=peer,
            )
    except SmokeError as error:
        add_case(
            cases,
            f"{peer} curl {'plaintext' if plaintext else 'mTLS'} evidence",
            False,
            str(error),
            origin=platform.node(),
            destination=peer,
        )


def remote_preflight(
    cases: list[dict[str, Any]], peer: str, remote_atm: str, expected_version: str
) -> str | None:
    """Fail fast when a peer is not already running and reachable.

    This never starts, stops, retries, or configures a remote daemon. In
    particular, an unseen macOS firewall dialog is reported as this bounded
    preflight failure instead of consuming time in a retry loop.
    """
    try:
        doctor = parse_json(remote_command(peer, remote_atm, ["doctor", "--json"]), f"{peer} doctor")
        ready = doctor_ready(doctor, expected_version)
        detail = (
            f"client={doctor.get('client_context', {}).get('version')}, "
            f"daemon={doctor.get('daemon_context', {}).get('version')}"
            if isinstance(doctor, dict)
            else "doctor response was not an object"
        )
        add_case(cases, f"{peer} doctor/version", ready, detail, origin=peer, destination=peer)
        if not ready:
            return None
        interfaces = parse_json(
            remote_command(peer, remote_atm, ["peer", "interface", "list", "--json"]),
            f"{peer} peer interface list",
        )
        host = advertised_host_from_json(interfaces)
        add_case(cases, f"{peer} advertised host", True, host, origin=peer, destination=peer)
        return host
    except SmokeError as error:
        add_case(cases, f"{peer} preflight", False, str(error), origin=peer, destination=peer)
        return None


def add_case(
    cases: list[dict[str, Any]],
    name: str,
    passed: bool,
    detail: str,
    *,
    origin: str | None = None,
    destination: str | None = None,
) -> None:
    """Record one observable check with the host where it started and ended."""
    local = platform.node()
    origin = origin or local
    destination = destination or origin
    cases.append(
        {
            "name": name,
            "status": "PASS" if passed else "FAIL",
            "detail": detail,
            "origin": origin,
            "destination": destination,
        }
    )
    print(f"{'PASS' if passed else 'FAIL'} {origin} -> {destination} {name}: {detail}", flush=True)


def artifact_segment(value: str, label: str) -> str:
    """Return a stable, path-safe run or host label."""
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", value):
        raise SmokeError(f"{label} must contain only letters, numbers, '.', '_', or '-'")
    return value


def operating_system_label() -> str:
    """Return the stable public OS label used by smoke evidence paths."""
    return {"darwin": "macos"}.get(platform.system().lower(), platform.system().lower())


def smoke_report_directory(feature: str) -> tuple[Path, dict[str, str]]:
    """Return an isolated public report directory for one live smoke run.

    This follows the fuzz-report principle of one self-contained evidence
    directory. Platform, host, and a process-qualified run ID make M5,
    Windows, and simultaneous local runs disjoint. Nothing is written to the
    site root or the top-level ``site/reports`` directory.
    """
    platform_label = artifact_segment(operating_system_label(), "local platform")
    host_label = artifact_segment(platform.node(), "local host name")
    requested_run_id = os.environ.get("ATM_SMOKE_RUN_ID", "").strip()
    run_id = artifact_segment(
        requested_run_id or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ"),
        "ATM_SMOKE_RUN_ID",
    )
    feature_label = artifact_segment(feature, "smoke feature")
    run_label = f"{run_id}-pid{os.getpid()}-{feature_label}"
    directory = ROOT / "site" / "reports" / "smoke" / platform_label / host_label / run_label
    return directory, {
        "feature": feature_label,
        "host": host_label,
        "platform": platform_label,
        "run_id": run_id,
    }


def send_read_ack(
    cases: list[dict[str, Any]],
    atm: str,
    identity: str,
    team: str,
    host: str,
    *,
    stage: str,
) -> None:
    # Keep the recipient identity and physical destination separate.  The CLI
    # canonicalizes this to the same wire target as `identity@team.host`, but
    # the explicit form makes each smoke lane's destination auditable.
    target = f"{identity}@{team}"
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    body = f"smoke-{host}-{stamp}"
    sent = command([atm, "send", target, body, "--host", host, "--json"])
    try:
        sent_id = message_id(parse_json(sent, f"send to {host}"))
        visible = wait_for_message(atm, team, sent_id)
        received = message_has_text(visible, body)
        add_case(cases, f"{stage} send/read/content", received, sent_id if received else "message body was not received exactly")
    except SmokeError as error:
        add_case(cases, f"{stage} send/read/content", False, str(error))
    required_body = f"smoke-ack-{host}-{stamp}"
    required = command(
        [atm, "send", target, required_body, "--host", host, "--requires-ack", "--json"]
    )
    try:
        required_id = message_id(parse_json(required, f"ack-required send to {host}"))
    except SmokeError as error:
        add_case(cases, f"{stage} requires-ack delivery/content", False, str(error))
        return
    message = wait_for_message(atm, team, required_id)
    pending = bool(message and message.get("requires_ack", message.get("requiresAck")) is True)
    received = pending and message_has_text(message, required_body)
    add_case(
        cases,
        f"{stage} requires-ack delivery/content",
        received,
        required_id if received else "pending acknowledgement message or body was not received",
    )
    if not received:
        return
    reply_body = f"smoke acknowledgement {stamp}"
    acknowledgement = command([atm, "ack", "--team", team, required_id, reply_body, "--json"])
    try:
        reply_id = reply_message_id(parse_json(acknowledgement, f"acknowledgement of {required_id}"))
    except SmokeError as error:
        add_case(cases, f"{stage} acknowledgement reply delivery/content", False, str(error))
        return
    reply = wait_for_message(atm, team, reply_id)
    reply_received = (
        message_has_text(reply, reply_body)
        and reply is not None
        and reply.get("acknowledgesMessageId", reply.get("acknowledges_message_id")) == required_id
    )
    add_case(
        cases,
        f"{stage} acknowledgement reply delivery/content",
        reply_received,
        reply_id if reply_received else "acknowledgement reply was not delivered exactly",
    )


def crosshost_send(
    cases: list[dict[str, Any]], atm: str, remote_atm: str, identity: str, team: str,
    remote_identity: str, remote_team: str, peer: str, remote_host: str, local_host: str,
) -> None:
    """Stage one: prove ordinary ATM delivery/read in each peer direction."""
    target = f"{remote_identity}@{remote_team}.{remote_host}"
    body = f"smoke-crosshost-send-{peer}-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
    try:
        sent_id = message_id(parse_json(command([atm, "send", target, body, "--json"]), f"send to {peer}"))
        remote = parse_json(
            remote_command(peer, remote_atm, ["read", "--team", remote_team, "--message-id", sent_id, "--json"]),
            f"{peer} read {sent_id}",
        )
        received = selected_message(remote, sent_id)
        passed = message_has_text(received, body)
        add_case(
            cases,
            f"{peer} crosshost send/read/content",
            passed,
            sent_id if passed else "remote read did not return the sent ULID and exact body",
            origin=platform.node(),
            destination=peer,
        )
    except SmokeError as error:
        add_case(cases, f"{peer} crosshost send/read/content", False, str(error), origin=platform.node(), destination=peer)
    reverse_target = f"{identity}@{team}.{local_host}"
    reverse_body = f"smoke-crosshost-send-reverse-{peer}-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}"
    try:
        sent_id = message_id(
            parse_json(
                remote_command(peer, remote_atm, ["send", reverse_target, reverse_body, "--json"]),
                f"{peer} send to local",
            )
        )
        received = wait_for_message(atm, team, sent_id)
        passed = message_has_text(received, reverse_body)
        add_case(
            cases,
            f"{peer} reverse crosshost send/read/content",
            passed,
            sent_id if passed else "local read did not return the sent ULID and exact body",
            origin=peer,
            destination=platform.node(),
        )
    except SmokeError as error:
        add_case(cases, f"{peer} reverse crosshost send/read/content", False, str(error), origin=peer, destination=platform.node())


def crosshost_ack(
    cases: list[dict[str, Any]], atm: str, remote_atm: str, identity: str, team: str,
    remote_identity: str, remote_team: str, peer: str, remote_host: str, local_host: str,
) -> None:
    """Stage two: prove required-ack delivery and reply in each peer direction."""
    target = f"{remote_identity}@{remote_team}.{remote_host}"
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    body = f"smoke-crosshost-ack-{peer}-{stamp}"
    try:
        sent_id = message_id(
            parse_json(
                command([atm, "send", target, body, "--requires-ack", "--json"]),
                f"ack-required send to {peer}",
            )
        )
        remote = parse_json(
            remote_command(peer, remote_atm, ["read", "--team", remote_team, "--message-id", sent_id, "--json"]),
            f"{peer} read {sent_id}",
        )
        received = selected_message(remote, sent_id)
        pending = bool(received and received.get("requires_ack", received.get("requiresAck")) is True)
        received_exactly = pending and message_has_text(received, body)
        add_case(
            cases,
            f"{peer} crosshost requires-ack delivery/content",
            received_exactly,
            sent_id if received_exactly else "remote read did not return the pending message and exact body",
            origin=platform.node(),
            destination=peer,
        )
        if not received_exactly:
            return
        reply_body = f"smoke-crosshost-reply-{peer}-{stamp}"
        acknowledgement = parse_json(
            remote_command(peer, remote_atm, ["ack", "--team", remote_team, sent_id, reply_body, "--json"]),
            f"{peer} acknowledgement of {sent_id}",
        )
        reply_id = reply_message_id(acknowledgement)
        reply = wait_for_message(atm, team, reply_id)
        passed = (
            message_has_text(reply, reply_body)
            and reply is not None
            and reply.get("acknowledgesMessageId", reply.get("acknowledges_message_id")) == sent_id
        )
        add_case(
            cases,
            f"{peer} crosshost acknowledgement reply",
            passed,
            reply_id if passed else "local read did not return the exact acknowledgement reply",
            origin=peer,
            destination=platform.node(),
        )
    except SmokeError as error:
        add_case(cases, f"{peer} crosshost requires-ack", False, str(error), origin=platform.node(), destination=peer)
    reverse_target = f"{identity}@{team}.{local_host}"
    reverse_body = f"smoke-crosshost-ack-reverse-{peer}-{stamp}"
    try:
        sent_id = message_id(
            parse_json(
                remote_command(peer, remote_atm, ["send", reverse_target, reverse_body, "--requires-ack", "--json"]),
                f"{peer} ack-required send to local",
            )
        )
        received = wait_for_message(atm, team, sent_id)
        pending = bool(received and received.get("requires_ack", received.get("requiresAck")) is True)
        delivered = pending and message_has_text(received, reverse_body)
        add_case(
            cases,
            f"{peer} reverse crosshost requires-ack delivery/content",
            delivered,
            sent_id if delivered else "local read did not return the pending message and exact body",
            origin=peer,
            destination=platform.node(),
        )
        if not delivered:
            return
        reply_body = f"smoke-crosshost-reverse-reply-{peer}-{stamp}"
        acknowledgement = parse_json(
            command([atm, "ack", "--team", team, sent_id, reply_body, "--json"]),
            f"local acknowledgement of {sent_id}",
        )
        reply_id = reply_message_id(acknowledgement)
        remote = parse_json(
            remote_command(peer, remote_atm, ["read", "--team", remote_team, "--message-id", reply_id, "--json"]),
            f"{peer} read acknowledgement {reply_id}",
        )
        reply = selected_message(remote, reply_id)
        passed = (
            message_has_text(reply, reply_body)
            and reply is not None
            and reply.get("acknowledgesMessageId", reply.get("acknowledges_message_id")) == sent_id
        )
        add_case(
            cases,
            f"{peer} reverse crosshost acknowledgement reply",
            passed,
            reply_id if passed else "remote read did not return the exact acknowledgement reply",
            origin=platform.node(),
            destination=peer,
        )
    except SmokeError as error:
        add_case(cases, f"{peer} reverse crosshost requires-ack", False, str(error), origin=peer, destination=platform.node())


def write_report(feature: str, cases: list[dict[str, Any]]) -> Path:
    directory, identity = smoke_report_directory(feature)
    host = identity["host"]
    directory.mkdir(parents=True, exist_ok=True)
    report = directory / f"{identity['feature']}.json"
    passed = all(case["status"] == "PASS" for case in cases)
    report.write_text(
        json.dumps(
            {
                **identity,
                "status": "PASS" if passed else "FAIL",
                "cases": cases,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    hosts = list(dict.fromkeys(case["origin"] for case in cases))
    for destination in (case["destination"] for case in cases):
        if destination not in hosts:
            hosts.append(destination)
    panes: list[tuple[str, Path]] = []
    for evidence_host in hosts:
        pane = directory / f"{artifact_segment(evidence_host, 'evidence host')}-{feature}.xhtml"
        compose(
            PANE_TEMPLATE,
            {
                "title": f"ATM smoke — {evidence_host}",
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "host": evidence_host,
                "body_html": render_feature_pane(feature, cases, evidence_host),
            },
            pane,
        )
        panes.append((evidence_host, pane))
    local_pane = next((path for evidence_host, path in panes if evidence_host == host), panes[0][1])
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-frame.html.j2",
        {
            "title": f"ATM smoke — {feature}",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_src": local_pane.name,
        },
        report.with_suffix(".html"),
    )
    pane_html = "\n".join(
        "<section><h2>{label}</h2><iframe title=\"{title}\" src=\"{source}\"></iframe></section>".format(
            label=escape(evidence_host),
            title=escape(f"ATM smoke evidence for {evidence_host}", quote=True),
            source=escape(path.name, quote=True),
        )
        for evidence_host, path in panes
    )
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-review.html.j2",
        {
            "title": "ATM cross-host smoke",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_html": pane_html,
        },
        directory / "index.html",
    )
    reports_root = ROOT / "site" / "reports"
    envelope = directory / "smoke.envelope.json"
    envelope.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "report_type": "smoke",
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "host_label": host,
                "report_html": (directory / "index.html").relative_to(reports_root).as_posix(),
                "status": "PASS" if passed else "FAIL",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    update_master_report_index()
    return report


def update_master_report_index() -> None:
    """Register every completed smoke run in the shared reports navigation."""
    completed = subprocess.run(
        [sys.executable, str(ROOT / ".just" / "generate_report_index.py"), "--root", str(ROOT)],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "unknown report-index error"
        raise SmokeError(f"failed to update the master smoke report index: {detail}")


def smoke_repetitions() -> int:
    """Return the required consecutive-attempt count for live smoke evidence."""
    value = os.environ.get("ATM_SMOKE_REPETITIONS", str(DEFAULT_LIVE_REPETITIONS))
    try:
        repetitions = int(value)
    except ValueError as error:
        raise SmokeError("ATM_SMOKE_REPETITIONS must be a positive integer") from error
    if not 1 <= repetitions <= 50:
        raise SmokeError("ATM_SMOKE_REPETITIONS must be between 1 and 50")
    return repetitions


def run_live_attempt(feature: str, peers: list[str]) -> list[dict[str, Any]]:
    atm, identity, team = require_environment()
    cases: list[dict[str, Any]] = []
    doctor = command([atm, "doctor", "--json"])
    try:
        report = parse_json(doctor, "doctor")
        expected_version = branch_version()
        daemon_version = report.get("daemon_context", {}).get("version")
        healthy = doctor_ready(report, expected_version)
        detail = (
            f"READY · ATM {daemon_version}"
            if healthy
            else f"expected={expected_version}; cli={report.get('client_context', {}).get('version')}; daemon={daemon_version}"
        )
        add_case(cases, "doctor", healthy, detail)
    except SmokeError as error:
        add_case(cases, "doctor", False, str(error))
    if not all(case["status"] == "PASS" for case in cases):
        return cases
    if feature == LOCALHOST:
        # This lane must exercise the portable loopback name, never an
        # advertised hostname or interface address from the current machine.
        send_read_ack(cases, atm, identity, team, "localhost", stage="localhost")
    elif feature == LOCAL_IP:
        try:
            local_ip_host = local_advertised_ipv4(advertised_host(atm))
            add_case(cases, "advertised local IPv4", True, local_ip_host)
            send_read_ack(cases, atm, identity, team, local_ip_host, stage="local-IP")
        except SmokeError as error:
            add_case(cases, "local-IP", False, str(error))
    else:
        try:
            physical_host = advertised_host(atm)
            add_case(cases, "advertised host", True, physical_host)
            send_read_ack(cases, atm, identity, team, physical_host, stage="local-IP")
        except SmokeError as error:
            add_case(cases, "local-IP", False, str(error))
            physical_host = ""
        # A bare loopback IP may legitimately resolve to several trusted test
        # peers.  That ambiguity must remain a fail-closed CLI contract; the
        # preflight needs the canonical loopback route instead.
        send_read_ack(cases, atm, identity, team, "localhost", stage="canonical-localhost")
        remote_atm = os.environ.get("ATM_SMOKE_REMOTE_ATM", "atm")
        expected_version = branch_version()
        try:
            remote_identity, remote_team = remote_context()
        except SmokeError as error:
            add_case(cases, "crosshost recipient identity", False, str(error))
            remote_identity = ""
            remote_team = ""
        if not peers:
            add_case(cases, "crosshost peers", False, "supply one or more SSH hostnames")
        for peer in peers:
            if not remote_identity or not remote_team:
                continue
            remote_host = remote_preflight(cases, peer, remote_atm, expected_version)
            if remote_host is None or not physical_host or feature == PEER_PREFLIGHT:
                continue
            if feature == CROSSHOST_CURL_PLAINTEXT:
                curl_doctor(cases, peer, atm, remote_atm, remote_host, expected_version, plaintext=True)
                continue
            if feature == CROSSHOST_CURL_MTLS:
                curl_doctor(cases, peer, atm, remote_atm, remote_host, expected_version, plaintext=False)
                continue
            crosshost_send(
                cases,
                atm,
                remote_atm,
                identity,
                team,
                remote_identity,
                remote_team,
                peer,
                remote_host,
                physical_host,
            )
            if feature == CROSSHOST_ACK:
                crosshost_ack(
                    cases,
                    atm,
                    remote_atm,
                    identity,
                    team,
                    remote_identity,
                    remote_team,
                    peer,
                    remote_host,
                    physical_host,
                )
    return cases


def run_live(feature: str, peers: list[str]) -> int:
    """Run each live ladder check repeatedly; one pass cannot hide a flake."""
    cases: list[dict[str, Any]] = []
    repetitions = smoke_repetitions()
    for attempt in range(1, repetitions + 1):
        print(f"LIVE SMOKE ATTEMPT {attempt}/{repetitions}", flush=True)
        attempt_cases = run_live_attempt(feature, peers)
        for case in attempt_cases:
            case["attempt"] = attempt
        cases.extend(attempt_cases)
    report = write_report(feature, cases)
    passed = all(case["status"] == "PASS" for case in cases)
    print(f"{'PASS' if passed else 'FAIL'} evidence: {report}")
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feature", nargs="?", default="normal")
    parser.add_argument("peers", nargs="*")
    args = parser.parse_args()
    if args.feature == ADMISSION_CAPACITY:
        if args.peers:
            raise SmokeError("admission-capacity does not accept peer hostnames")
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts" / "smoke" / "run_admission_capacity.py")],
            check=False,
        ).returncode
    if args.feature in FIXTURE_FEATURES:
        if args.peers:
            raise SmokeError(f"fixture smoke `{args.feature}` does not accept hostnames")
        return subprocess.run([sys.executable, str(ROOT / "scripts" / "smoke" / "run.py"), args.feature, "--write-artifacts"], check=False).returncode
    feature = LOCAL_IP if args.feature == LOCAL_IP_ALIAS else args.feature
    # `crosshost` remains a compatibility alias for the first explicit
    # cross-host stage; new automation should use `crosshost-send`.
    feature = CROSSHOST_SEND if feature == CROSSHOST else feature
    crosshost_features = {
        PEER_PREFLIGHT,
        CROSSHOST_SEND,
        CROSSHOST_ACK,
        CROSSHOST_CURL_PLAINTEXT,
        CROSSHOST_CURL_MTLS,
    }
    if feature not in {LOCALHOST, LOCAL_IP, *crosshost_features}:
        raise SmokeError(
            "supported smoke features: fast, normal, thorough, localhost, local-ip, "
            "peer-preflight, crosshost-curl-plain, crosshost-curl-tls, "
            "crosshost-send, crosshost-ack, admission-capacity"
        )
    if args.peers and feature not in crosshost_features:
        raise SmokeError("hostnames are only valid with a cross-host smoke feature")
    if feature in crosshost_features and not args.peers:
        raise SmokeError(f"smoke `{feature}` requires one or more SSH hostnames")
    return run_live(feature, args.peers)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

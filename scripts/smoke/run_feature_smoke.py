#!/usr/bin/env python3
"""Run one progressively stronger smoke feature against the selected daemon.

The runner never starts, stops, switches, or configures a daemon.  Use the
daemon-switch skill before invoking it.  Local identity comes from the normal
CLI environment: ``ATM_IDENTITY`` and ``ATM_TEAM``.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
import os
from pathlib import Path
import platform
import re
import shlex
import subprocess
import sys
import tempfile
import time
from typing import Any

from run_inbound_peer_smoke import PANE_TEMPLATE, compose


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_FEATURES = frozenset({"fast", "normal", "thorough"})
LOCALHOST = "localhost"
LOCAL_IP = "local-ip"
LOCAL_IP_ALIAS = "local-up"
LOOPBACK_IP = "127.0.0.1"
CROSSHOST = "crosshost"
PEER_PREFLIGHT = "peer-preflight"
CROSSHOST_SEND = "crosshost-send"
CROSSHOST_ACK = "crosshost-ack"
CROSSHOST_CURL_PLAINTEXT = "crosshost-curl-plain"
CROSSHOST_CURL_MTLS = "crosshost-curl-tls"
DOCTOR_BODY = '{"home_dir":"","current_dir":"","team_override":null,"caller_team":null,"caller_identity":null}'


class SmokeError(RuntimeError):
    """A smoke prerequisite or assertion failed."""


def command(argv: list[str], timeout: float = 15.0) -> dict[str, Any]:
    try:
        completed = subprocess.run(argv, text=True, capture_output=True, check=False, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"argv": argv, "exit_code": None, "stdout": "", "stderr": str(error)}
    return {"argv": argv, "exit_code": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr}


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
    versions = {package.get("version") for package in packages if package.get("name") in {"atm", "atm-daemon"}}
    if len(versions) != 1 or not isinstance(next(iter(versions)), str):
        raise SmokeError("cargo metadata did not expose one shared atm/atm-daemon version")
    return next(iter(versions))


def message_id(value: Any) -> str:
    if isinstance(value, dict):
        for key in ("message_id", "messageId"):
            if isinstance(value.get(key), str) and value[key]:
                return value[key]
        for child in value.values():
            try:
                return message_id(child)
            except SmokeError:
                continue
    if isinstance(value, list):
        for child in value:
            try:
                return message_id(child)
            except SmokeError:
                continue
    raise SmokeError("send JSON did not contain a message ID")


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


def advertised_host_from_json(interfaces: Any) -> str:
    """Extract an enabled advertised host from the public interface response."""
    stack = [interfaces]
    while stack:
        current = stack.pop()
        if isinstance(current, dict):
            host = current.get("advertise_host", current.get("advertised_host"))
            if current.get("enabled") is not False and isinstance(host, str) and host:
                return host
            stack.extend(current.values())
        elif isinstance(current, list):
            stack.extend(current)
    raise SmokeError("peer interface list JSON has no enabled advertised host")


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
            remote_id = artifact_segment(f"{platform.node()}-{peer}", "curl peer label")
            remote_local_ca = f"/tmp/atm-smoke-{remote_id}-local.pem"
            remote_public_path = f"/tmp/atm-smoke-{remote_id}-peer.pem"
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
            add_case(cases, f"{peer} curl {'plaintext' if plaintext else 'mTLS'} to local doctor", doctor_ready(remote_report, expected_version), "HTTP 200 real daemon doctor" if doctor_ready(remote_report, expected_version) else "doctor response was not healthy/ready")
            local_curl = ["curl", "--silent", "--show-error", "--fail", "--connect-timeout", "2", "--max-time", "5", "-X", "GET", *headers]
            if not plaintext:
                local_curl.extend(["--cert", local_bundle, "--cacert", str(remote_public)])
            local_curl.extend(["--resolve", f"{remote_authority}:43101:{remote_host}", "--data", DOCTOR_BODY, remote_url])
            local_result = command(local_curl)
            local_report = parse_json(local_result, f"curl doctor to {peer}")
            add_case(cases, f"local curl {'plaintext' if plaintext else 'mTLS'} to {peer} doctor", doctor_ready(local_report, expected_version), "HTTP 200 real daemon doctor" if doctor_ready(local_report, expected_version) else "doctor response was not healthy/ready")
    except SmokeError as error:
        add_case(cases, f"{peer} curl {'plaintext' if plaintext else 'mTLS'} evidence", False, str(error))


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
        add_case(cases, f"{peer} doctor/version", ready, detail)
        if not ready:
            return None
        interfaces = parse_json(
            remote_command(peer, remote_atm, ["peer", "interface", "list", "--json"]),
            f"{peer} peer interface list",
        )
        host = advertised_host_from_json(interfaces)
        add_case(cases, f"{peer} advertised host", True, host)
        return host
    except SmokeError as error:
        add_case(cases, f"{peer} preflight", False, str(error))
        return None


def add_case(cases: list[dict[str, Any]], name: str, passed: bool, detail: str) -> None:
    cases.append({"name": name, "status": "PASS" if passed else "FAIL", "detail": detail})
    print(f"{'PASS' if passed else 'FAIL'} {name}: {detail}", flush=True)


def render_feature_pane(feature: str, cases: list[dict[str, Any]]) -> str:
    """Render exactly the progressive live checks executed by this invocation."""
    doctor = next((case for case in cases if case["name"] == "doctor"), None)
    executed = [case for case in cases if case["name"] != "doctor"]
    rows = "".join(
        "<tr class=\"{status}\"><td>{marker}</td><td>{name}</td><td>{detail}</td></tr>".format(
            status="pass" if case["status"] == "PASS" else "fail",
            marker="✓" if case["status"] == "PASS" else "✗",
            name=escape(case["name"]),
            detail=escape(case["detail"]),
        )
        for case in executed
    )
    logs = "".join(
        f"<li><strong>{escape(case['name'])}</strong>: {escape(case['status'])}</li>" for case in executed
    )
    failures = [case["name"] for case in executed if case["status"] == "FAIL"]
    preflight = (
        f"<h2>Preflight</h2><p class=\"{'pass' if doctor['status'] == 'PASS' else 'fail'}\">"
        f"<strong>Doctor {'passed' if doctor['status'] == 'PASS' else 'failed'}.</strong><br />"
        f"{escape(doctor['detail']).replace(chr(10), '<br />')}</p>"
        if doctor
        else "<h2>Preflight</h2><p class=\"fail\">Doctor was not executed.</p>"
    )
    assessment = (
        "Investigation required: " + "; ".join(failures)
        if failures
        else "No issues found by executed checks."
    )
    return (
        f"<h1>ATM smoke: {escape(feature)}</h1>"
        f"{preflight}"
        f"<table><thead><tr><th>Status</th><th>Test case</th><th>Result / message ID</th></tr>"
        f"</thead><tbody>{rows}</tbody></table>"
        f"<h2>Session log</h2><ul>{logs}</ul>"
        f"<h2>Assessment</h2><p class=\"assessment\">{escape(assessment)}</p>"
    )


def artifact_segment(value: str, label: str) -> str:
    """Return a stable, path-safe run or host label."""
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", value):
        raise SmokeError(f"{label} must contain only letters, numbers, '.', '_', or '-'")
    return value


def send_read_ack(
    cases: list[dict[str, Any]],
    atm: str,
    identity: str,
    team: str,
    host: str,
    *,
    stage: str,
) -> None:
    target = f"{identity}@{team}.{host}"
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    body = f"smoke-{host}-{stamp}"
    sent = command([atm, "send", target, body, "--json"])
    try:
        sent_id = message_id(parse_json(sent, f"send to {host}"))
        visible = wait_for_message(atm, team, sent_id)
        received = message_has_text(visible, body)
        add_case(cases, f"{stage} send/read/content", received, sent_id if received else "message body was not received exactly")
    except SmokeError as error:
        add_case(cases, f"{stage} send/read/content", False, str(error))
    required_body = f"smoke-ack-{host}-{stamp}"
    required = command([atm, "send", target, required_body, "--requires-ack", "--json"])
    try:
        required_id = message_id(parse_json(required, f"ack-required send to {host}"))
        message = wait_for_message(atm, team, required_id)
        pending = bool(message and message.get("requires_ack", message.get("requiresAck")) is True)
        received = pending and message_has_text(message, required_body)
        add_case(cases, f"{stage} requires-ack delivery/content", received, required_id if received else "pending acknowledgement message or body was not received")
        if pending:
            reply_body = f"smoke acknowledgement {stamp}"
            acknowledgement = command([atm, "ack", "--team", team, required_id, reply_body, "--json"])
            reply_id = reply_message_id(parse_json(acknowledgement, f"acknowledgement of {required_id}"))
            reply = wait_for_message(atm, team, reply_id)
            reply_received = (
                message_has_text(reply, reply_body)
                and reply is not None
                and reply.get("acknowledgesMessageId", reply.get("acknowledges_message_id")) == required_id
            )
            add_case(cases, f"{stage} acknowledgement reply delivery/content", reply_received, reply_id if reply_received else "acknowledgement reply was not delivered exactly")
    except SmokeError as error:
        add_case(cases, f"{stage} requires-ack delivery/content", False, str(error))


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
        )
    except SmokeError as error:
        add_case(cases, f"{peer} crosshost send/read/content", False, str(error))
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
        )
    except SmokeError as error:
        add_case(cases, f"{peer} reverse crosshost send/read/content", False, str(error))


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
        )
        if not received_exactly:
            return
        reply_body = f"smoke-crosshost-reply-{peer}-{stamp}"
        acknowledgement = parse_json(
            remote_command(peer, remote_atm, ["ack", "--team", team, sent_id, reply_body, "--json"]),
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
        )
    except SmokeError as error:
        add_case(cases, f"{peer} crosshost requires-ack", False, str(error))
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
        )
    except SmokeError as error:
        add_case(cases, f"{peer} reverse crosshost requires-ack", False, str(error))


def write_report(feature: str, cases: list[dict[str, Any]]) -> Path:
    run_id = artifact_segment(
        os.environ.get("ATM_SMOKE_RUN_ID", "").strip()
        or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ"),
        "ATM_SMOKE_RUN_ID",
    )
    host = artifact_segment(platform.node(), "local host name")
    directory = ROOT / "reports" / "smoke" / run_id
    directory.mkdir(parents=True, exist_ok=True)
    report = directory / f"{host}-{feature}.json"
    passed = all(case["status"] == "PASS" for case in cases)
    report.write_text(json.dumps({"feature": feature, "status": "PASS" if passed else "FAIL", "cases": cases}, indent=2) + "\n", encoding="utf-8")
    # Reuse the established AI.21-pre XHTML pane template instead of creating
    # a second report format for the same live-daemon evidence.
    pane = report.with_suffix(".xhtml")
    compose(
        PANE_TEMPLATE,
        {
            "title": f"ATM smoke — {feature}",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "host": host,
            "body_html": render_feature_pane(feature, cases),
        },
        pane,
    )
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-frame.html.j2",
        {
            "title": f"ATM smoke — {feature}",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_src": pane.name,
        },
        report.with_suffix(".html"),
    )
    panes = sorted(path for path in directory.glob("*.xhtml") if path.is_file())
    pane_html = "\n".join(
        "<section><h2>{label}</h2><iframe title=\"{title}\" src=\"{source}\"></iframe></section>".format(
            label=escape(path.name),
            title=escape(path.stem, quote=True),
            source=escape(path.name, quote=True),
        )
        for path in panes
    )
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-review.html.j2",
        {
            "title": "ATM smoke evidence",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_html": pane_html,
        },
        directory / "index.html",
    )
    return report


def run_live(feature: str, peers: list[str]) -> int:
    atm, identity, team = require_environment()
    cases: list[dict[str, Any]] = []
    doctor = command([atm, "doctor", "--json"])
    try:
        report = parse_json(doctor, "doctor")
        expected_version = branch_version()
        client_version = report.get("client_context", {}).get("version")
        daemon_version = report.get("daemon_context", {}).get("version")
        healthy = (
            report.get("summary", {}).get("status") == "healthy"
            and report.get("runtime_status", {}).get("readiness") == "ready"
            and client_version == expected_version
            and daemon_version == expected_version
        )
        detail = (
            "status: healthy\n"
            "readiness: ready\n"
            f"expected version: {expected_version}\n"
            f"CLI version: {client_version}\n"
            f"daemon version: {daemon_version}"
            if healthy
            else (
                f"expected version: {expected_version}\n"
                f"CLI version: {client_version}\n"
                f"daemon version: {daemon_version}"
            )
        )
        add_case(cases, "doctor", healthy, detail)
    except SmokeError as error:
        add_case(cases, "doctor", False, str(error))
    if not all(case["status"] == "PASS" for case in cases):
        print(f"FAIL evidence: {write_report(feature, cases)}")
        return 1
    # The first live stage deliberately targets the daemon's advertised
    # physical interface.  It must be an ordinary host-qualified peer send;
    # DNS `localhost` and a direct in-process route would hide that contract.
    try:
        physical_host = advertised_host(atm)
        send_read_ack(
            cases,
            atm,
            identity,
            team,
            physical_host,
            stage="physical-interface",
        )
    except SmokeError as error:
        add_case(cases, "physical-interface", False, str(error))
    if feature == LOCALHOST:
        pass
    elif feature == LOCAL_IP:
        send_read_ack(cases, atm, identity, team, LOOPBACK_IP, stage="loopback-IP")
    else:
        send_read_ack(cases, atm, identity, team, LOOPBACK_IP, stage="loopback-IP")
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
            if remote_host is None or feature == PEER_PREFLIGHT:
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
    report = write_report(feature, cases)
    passed = all(case["status"] == "PASS" for case in cases)
    print(f"{'PASS' if passed else 'FAIL'} evidence: {report}")
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feature", nargs="?", default="normal")
    parser.add_argument("peers", nargs="*")
    args = parser.parse_args()
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
            "crosshost-send, crosshost-ack"
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

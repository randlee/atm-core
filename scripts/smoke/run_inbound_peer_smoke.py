#!/usr/bin/env python3
"""Prove inbound cross-host writes against an already-running local daemon.

The runner never starts, stops, switches, or configures a daemon.  For each
configured SSH peer it asks that peer's public ``atm`` CLI to send two writes
to this host, then proves the exact IDs are visible through this host's public
``atm read`` command.  It emits one concise PASS/FAIL line per phase and saves
sanitized bounded command and daemon-log evidence on every outcome.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import time
from typing import Any

from smoke_common import (
    SmokeError,
    command_result,
    extract_advertised_host,
    extract_message_id,
    sanitize,
)

REQUIRED_LOCAL_CHECKS = frozenset({"localhost/local loopback", "own-IP", "nudge"})

def require_argv(value: Any, field: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        raise SmokeError(f"{field} must be a non-empty argv array")
    return list(value)


def require_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SmokeError(f"{field} must be a non-empty string")
    return value


def load_config(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SmokeError(f"cannot read config {path}: {error}") from error
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise SmokeError("config must be an object with schema_version 1")
    local = value.get("local")
    if not isinstance(local, dict):
        raise SmokeError("local must be an object")
    require_argv(local.get("atm_command"), "local.atm_command")
    require_string(local.get("identity"), "local.identity")
    require_string(local.get("team"), "local.team")
    require_string(local.get("expected_daemon_version"), "local.expected_daemon_version")
    if not isinstance(local.get("expected_http_api_version"), int):
        raise SmokeError("local.expected_http_api_version must be an integer")
    if "advertised_host" in local:
        require_string(local["advertised_host"], "local.advertised_host")
    if "log_command" in local:
        require_argv(local["log_command"], "local.log_command")
    peers = value.get("peers", [])
    if not isinstance(peers, list) or (not peers and not isinstance(value.get("host"), dict)):
        raise SmokeError("peers must be a non-empty array unless host mode is configured")
    for index, peer in enumerate(peers):
        if not isinstance(peer, dict):
            raise SmokeError(f"peers[{index}] must be an object")
        require_string(peer.get("name"), f"peers[{index}].name")
        require_argv(peer.get("ssh_command"), f"peers[{index}].ssh_command")
        require_argv(peer.get("atm_command"), f"peers[{index}].atm_command")
        require_string(peer.get("identity"), f"peers[{index}].identity")
        require_string(peer.get("team"), f"peers[{index}].team")
        require_string(peer.get("expected_daemon_version"), f"peers[{index}].expected_daemon_version")
        shell = peer.get("shell", "posix")
        if shell not in {"posix", "powershell"}:
            raise SmokeError(f"peers[{index}].shell must be posix or powershell")
        if "log_command" in peer:
            require_argv(peer["log_command"], f"peers[{index}].log_command")
    return value


def validate_host_config(config: dict[str, Any]) -> dict[str, Any]:
    """Validate the single-host role used independently on Mac, M5, and Windows."""
    host = config.get("host")
    if not isinstance(host, dict):
        raise SmokeError("host mode requires a `host` object")
    require_string(host.get("name"), "host.name")
    checks = host.get("local_checks", {})
    if not isinstance(checks, dict):
        raise SmokeError("host.local_checks must be an object")
    missing_checks = REQUIRED_LOCAL_CHECKS - checks.keys()
    if missing_checks:
        raise SmokeError(
            "host.local_checks is missing required check(s): "
            + ", ".join(sorted(missing_checks))
        )
    for name, command in checks.items():
        if name not in REQUIRED_LOCAL_CHECKS:
            raise SmokeError(f"unsupported host.local_checks key `{name}`")
        require_argv(command, f"host.local_checks.{name}")
    outbound = host.get("outbound_target")
    if outbound is not None:
        require_string(outbound, "host.outbound_target")
    return host


def shell_quote_powershell(argument: str) -> str:
    return "'" + argument.replace("'", "''") + "'"


def remote_command(peer: dict[str, Any], argv: list[str], environment: dict[str, str] | None = None) -> list[str]:
    """Build an SSH command without relying on the local shell."""
    shell = peer.get("shell", "posix")
    environment = environment or {}
    if shell == "posix":
        prefix = " ".join(f"{key}={shlex.quote(value)}" for key, value in environment.items())
        script = (prefix + " " if prefix else "") + shlex.join(argv)
        remote = ["sh", "-lc", script]
    else:
        assignments = "; ".join(f"$env:{key}={shell_quote_powershell(value)}" for key, value in environment.items())
        script = (assignments + "; " if assignments else "") + "& " + " ".join(shell_quote_powershell(item) for item in argv)
        remote = ["powershell", "-NoProfile", "-NonInteractive", "-Command", script]
    return require_argv(peer["ssh_command"], "peer.ssh_command") + remote


def local_command(local: dict[str, Any], argv: list[str]) -> list[str]:
    return require_argv(local["atm_command"], "local.atm_command") + argv


def message_from_read(raw: str) -> dict[str, Any] | None:
    try:
        value = json.loads(raw)
    except json.JSONDecodeError:
        return None
    if isinstance(value, dict):
        message = value.get("message")
        if isinstance(message, dict):
            return message
        messages = value.get("messages")
        if isinstance(messages, list) and len(messages) == 1 and isinstance(messages[0], dict):
            return messages[0]
    return None


def read_received(local: dict[str, Any], message_id: str, timeout: float, deadline: float) -> dict[str, Any]:
    command = local_command(local, ["read", "--team", local["team"], "--message-id", message_id, "--json"])
    latest: dict[str, Any] = {}
    while time.monotonic() < deadline:
        latest = command_result(command, timeout)
        message = message_from_read(latest["stdout"])
        if message and message.get("message_id", message.get("messageId")) == message_id:
            latest["message"] = message
            return latest
        time.sleep(0.4)
    return latest


def load_handoff(path: Path) -> tuple[str, list[dict[str, str]]]:
    """Read peer-published exact IDs; never search a mailbox by body/content."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SmokeError(f"cannot read handoff {path}: {error}") from error
    if not isinstance(value, dict):
        raise SmokeError(f"handoff {path} must be a JSON object")
    host = require_string(value.get("host"), f"handoff {path}.host")
    outbound = value.get("outbound")
    if not isinstance(outbound, list):
        raise SmokeError(f"handoff {path}.outbound must be an array")
    accepted: list[dict[str, str]] = []
    for item in outbound:
        if not isinstance(item, dict):
            raise SmokeError(f"handoff {path} has invalid outbound item")
        kind = item.get("kind")
        message_id = item.get("message_id")
        if kind not in {"remote incoming no-ack", "remote incoming requires-ack"} or not isinstance(message_id, str) or not message_id:
            raise SmokeError(f"handoff {path} has invalid outbound kind or message_id")
        accepted.append({"kind": kind, "message_id": message_id})
    if len({(item["kind"], item["message_id"]) for item in accepted}) != len(accepted):
        raise SmokeError(f"handoff {path} repeats an exact message ID")
    return host, accepted


def compact(label: str, passed: bool, detail: str) -> None:
    print(f"{'PASS' if passed else 'FAIL'} {label}: {detail}", flush=True)


def capture_log(command: list[str] | None, timeout: float) -> dict[str, Any] | None:
    return command_result(command, timeout) if command else None


PANE_CASES = (
    "doctor",
    "localhost/local loopback",
    "own-IP",
    "remote incoming no-ack",
    "remote incoming requires-ack",
    "ack reply",
    "nudge",
)


def doctor_summary(result: dict[str, Any] | None) -> str:
    if not result:
        return "not collected"
    if result.get("exit_code") != 0:
        return "unavailable: " + (result.get("stderr") or "doctor failed")
    try:
        value = json.loads(result.get("stdout", ""))
    except json.JSONDecodeError:
        return "doctor returned non-JSON"
    found: dict[str, Any] = {}

    def walk(item: Any) -> None:
        if isinstance(item, dict):
            for key, nested in item.items():
                if key in {"pid", "daemon_pid", "readiness", "daemon_version", "client_version", "version", "peer_wire_security", "http_api_version"} and key not in found:
                    found[key] = nested
                walk(nested)
        elif isinstance(item, list):
            for nested in item:
                walk(nested)

    walk(value)
    details = [f"{key}={found[key]}" for key in ("client_version", "daemon_version", "version", "http_api_version", "peer_wire_security", "pid", "daemon_pid", "readiness") if key in found]
    return ", ".join(details) if details else "doctor ready (version/PID fields absent)"


def doctor_field(result: dict[str, Any], *path: str) -> Any:
    """Read one stable field from the public doctor JSON response."""
    try:
        value: Any = json.loads(result.get("stdout", ""))
    except json.JSONDecodeError:
        return None
    for part in path:
        if not isinstance(value, dict):
            return None
        value = value.get(part)
    return value


def doctor_matches_expected(local: dict[str, Any], result: dict[str, Any]) -> tuple[bool, str]:
    """A usable daemon must report the expected release and HTTP API version."""
    if result["exit_code"] != 0:
        return False, result["stderr"] or "doctor failed"
    daemon_version = doctor_field(result, "daemon_context", "version")
    api_version = doctor_field(result, "daemon_context", "http_api_version")
    readiness = doctor_field(result, "runtime_status", "readiness")
    expected_version = local["expected_daemon_version"]
    expected_api = local["expected_http_api_version"]
    if daemon_version != expected_version:
        return False, f"daemon version {daemon_version!r} != expected {expected_version!r}"
    if not isinstance(api_version, str) or api_version.split(".", 1)[0] != str(expected_api):
        return False, f"HTTP API version {api_version!r} != expected {expected_api!r}"
    if readiness != "ready":
        return False, f"daemon readiness {readiness!r} != 'ready'"
    return True, f"daemon={daemon_version}, http_api={api_version}, readiness={readiness}"


def status_for(records: list[dict[str, Any]], phase: str) -> tuple[str, str]:
    record = next((item for item in records if item.get("phase") == phase), None)
    if record is None:
        return "not-run", "not run by this inbound-only runner"
    if record.get("passed"):
        return "pass", record.get("message_id") or "completed"
    result = record.get("result", {})
    return "fail", result.get("parse_error") or result.get("stderr") or "failed"


def render_host_pane(host: str, doctor: dict[str, Any] | None, rows: dict[str, tuple[str, str]], records: list[dict[str, Any]]) -> str:
    """Render escaped pane body; the repository sc-compose template wraps it."""
    table_rows = []
    for case in PANE_CASES:
        status, detail = rows.get(case, ("not-run", "not run"))
        marker = {"pass": "✓", "fail": "✗", "not-run": "—"}[status]
        table_rows.append(
            f"<tr class=\"{escape(status)}\"><td>{escape(marker)}</td><td>{escape(case)}</td>"
            f"<td>{escape(detail)}</td></tr>"
        )
    log_rows = "".join(
        f"<li><strong>{escape(str(record.get('phase', 'unknown')))}</strong>: "
        f"{escape('PASS' if record.get('passed') else 'FAIL')}</li>" for record in records
    ) or "<li>No commands recorded.</li>"
    failed = [record for record in records if record.get("passed") is False]
    not_run = [case for case, value in rows.items() if value[0] == "not-run"]
    if failed:
        assessment = "Investigation required: " + "; ".join(str(item.get("phase")) for item in failed)
    elif not_run:
        assessment = "No executed failure. Remaining investigation: " + ", ".join(not_run)
    else:
        assessment = "No issues found by executed checks."
    return f"""<h1>ATM peer smoke: {escape(host)}</h1><p><strong>Version / daemon:</strong> {escape(doctor_summary(doctor))}</p>
<table><thead><tr><th>Status</th><th>Test case</th><th>Result / message ID</th></tr></thead><tbody>{''.join(table_rows)}</tbody></table>
<h2>Session log</h2><ul>{log_rows}</ul><h2>Assessment</h2><p class=\"assessment\">{escape(assessment)}</p>"""


REPO_ROOT = Path(__file__).resolve().parents[2]
PANE_TEMPLATE = REPO_ROOT / "templates/smoke-report/inbound-peer-pane.xhtml.j2"
REVIEW_TEMPLATE = REPO_ROOT / "templates/smoke-report/inbound-peer-review.xhtml.j2"


def compose(template: Path, variables: dict[str, Any], output: Path) -> None:
    """Render only through the repository's sc-compose template mechanism."""
    with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
        json.dump(variables, handle)
        variables_path = Path(handle.name)
    try:
        result = command_result([
            "sc-compose", "render", "--root", str(REPO_ROOT), "--file", str(template),
            "--var-file", str(variables_path), "--output", str(output),
        ], 15.0)
        if result["exit_code"] != 0:
            raise SmokeError("sc-compose render failed: " + (result["stderr"] or result["stdout"]))
    finally:
        variables_path.unlink(missing_ok=True)


def write_host_panes(evidence_dir: Path, local: dict[str, Any], peers: list[dict[str, Any]], records: list[dict[str, Any]]) -> None:
    generated_at = datetime.now(timezone.utc).isoformat()
    local_doctor = next((item.get("result") for item in records if item.get("phase") == "local-doctor"), None)
    local_rows = {"doctor": status_for(records, "local-doctor")}
    for case in ("localhost/local loopback", "own-IP", "nudge", "ack reply"):
        local_rows[case] = status_for(records, case)
    for peer in peers:
        for kind in ("noack", "ack-required"):
            _, detail = status_for(records, f"{peer['name']}-read-{kind}")
            send_status, _ = status_for(records, f"{peer['name']}-send-{kind}")
            read_status, _ = status_for(records, f"{peer['name']}-read-{kind}")
            status = "pass" if send_status == read_status == "pass" else "fail" if "fail" in {send_status, read_status} else "not-run"
            local_rows[f"remote incoming {'no-ack' if kind == 'noack' else 'requires-ack'}"] = (status, detail)
    compose(PANE_TEMPLATE, {"title": "ATM peer smoke — local", "generated_at": generated_at, "host": "local", "body_html": render_host_pane("local", local_doctor, local_rows, records)}, evidence_dir / "local.xhtml")
    for peer in peers:
        doctor = next((item.get("result") for item in records if item.get("phase") == f"{peer['name']}-doctor"), None)
        rows = {"doctor": status_for(records, f"{peer['name']}-doctor")}
        for kind in ("noack", "ack-required"):
            send_status, send_detail = status_for(records, f"{peer['name']}-send-{kind}")
            read_status, read_detail = status_for(records, f"{peer['name']}-read-{kind}")
            status = "pass" if send_status == read_status == "pass" else "fail" if "fail" in {send_status, read_status} else "not-run"
            rows[f"remote incoming {'no-ack' if kind == 'noack' else 'requires-ack'}"] = (status, send_detail if status == "pass" else read_detail)
        peer_records = [item for item in records if item.get("phase", "").startswith(f"{peer['name']}-")]
        compose(PANE_TEMPLATE, {"title": f"ATM peer smoke — {peer['name']}", "generated_at": generated_at, "host": peer["name"], "body_html": render_host_pane(peer["name"], doctor, rows, peer_records)}, evidence_dir / f"{peer['name']}.xhtml")


def run_host(config: dict[str, Any], output_root: Path, timeout: float, receive_timeout: float, handoff_paths: list[Path]) -> int:
    """Run one host independently; peers run this before sharing their pane/handoff."""
    host = validate_host_config(config)
    local = config["local"]
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_dir = output_root / stamp
    evidence_dir.mkdir(parents=True, exist_ok=False)
    records: list[dict[str, Any]] = []
    doctor = command_result(local_command(local, ["doctor", "--json"]), timeout)
    doctor_ok, doctor_detail = doctor_matches_expected(local, doctor)
    records.append({"phase": "doctor", "result": doctor, "passed": doctor_ok, "detail": doctor_detail})
    compact("doctor", doctor_ok, doctor_detail)
    for name, command in host.get("local_checks", {}).items():
        result = command_result(command, timeout)
        passed = result["exit_code"] == 0
        records.append({"phase": name, "result": result, "passed": passed})
        compact(name, passed, "completed" if passed else result["stderr"] or "failed")
    handoff: list[dict[str, Any]] = []
    target = host.get("outbound_target")
    if target:
        for kind, needs_ack in (("remote incoming no-ack", False), ("remote incoming requires-ack", True)):
            args = ["send", target, f"inbound-smoke-{host['name']}-{kind}-{stamp}", "--json"]
            if needs_ack:
                args.insert(-1, "--requires-ack")
            result = command_result(local_command(local, args), timeout)
            try:
                message_id = extract_message_id(result["stdout"]) if result["exit_code"] == 0 else None
            except SmokeError as error:
                message_id = None
                result["parse_error"] = str(error)
            passed = bool(message_id)
            records.append({"phase": kind, "result": result, "message_id": message_id, "passed": passed})
            handoff.append({"kind": kind, "message_id": message_id, "passed": passed})
            compact(kind, passed, message_id or result.get("parse_error", result["stderr"] or "failed"))
    for handoff_path in handoff_paths:
        peer, outbound_ids = load_handoff(handoff_path)
        for item in outbound_ids:
            message_id = item["message_id"]
            result = read_received(local, message_id, timeout, time.monotonic() + receive_timeout)
            message = result.get("message")
            requires_ack = item["kind"] == "remote incoming requires-ack"
            passed = bool(message) and (not requires_ack or message.get("requires_ack") is True)
            phase = f"{item['kind']}:{peer}"
            records.append({"phase": phase, "result": result, "message_id": message_id, "passed": passed})
            compact(phase, passed, message_id if passed else result.get("stderr") or "exact ID not visible locally")
    rows = {case: status_for(records, case) for case in PANE_CASES}
    for case in ("remote incoming no-ack", "remote incoming requires-ack"):
        matching = [item for item in records if item.get("phase", "").startswith(case + ":")]
        if matching:
            passed = all(item.get("passed") for item in matching)
            rows[case] = ("pass" if passed else "fail", ", ".join(str(item.get("message_id", "")) for item in matching))
    logs = {"local": capture_log(local.get("log_command"), timeout)}
    (evidence_dir / "handoff.json").write_text(json.dumps({"host": host["name"], "generated_at": datetime.now(timezone.utc).isoformat(), "outbound": handoff}, indent=2) + "\n", encoding="utf-8")
    (evidence_dir / "results.json").write_text(json.dumps({"status": "pass" if all(item.get("passed") for item in records) else "fail", "records": records, "logs": logs}, indent=2) + "\n", encoding="utf-8")
    compose(PANE_TEMPLATE, {"title": f"ATM peer smoke — {host['name']}", "generated_at": datetime.now(timezone.utc).isoformat(), "host": host["name"], "body_html": render_host_pane(host["name"], doctor, rows, records)}, evidence_dir / f"{host['name']}.xhtml")
    passed = all(item.get("passed") for item in records)
    compact("evidence", passed, str(evidence_dir))
    return 0 if passed else 1


def run(config: dict[str, Any], output_root: Path, timeout: float, receive_timeout: float) -> int:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_dir = output_root / stamp
    evidence_dir.mkdir(parents=True, exist_ok=False)
    local = config["local"]
    records: list[dict[str, Any]] = []
    all_passed = True

    doctor = command_result(local_command(local, ["doctor", "--json"]), timeout)
    local_ready, doctor_detail = doctor_matches_expected(local, doctor)
    compact("local-doctor", local_ready, doctor_detail)
    records.append({"phase": "local-doctor", "result": doctor, "passed": local_ready})
    all_passed &= local_ready
    host = config.get("host")
    if isinstance(host, dict):
        for name, command in validate_host_config(config).get("local_checks", {}).items():
            result = command_result(command, timeout)
            passed = result["exit_code"] == 0
            records.append({"phase": name, "result": result, "passed": passed})
            compact(name, passed, "completed" if passed else result["stderr"] or "failed")
            all_passed &= passed
    advertised_host = local.get("advertised_host")
    if not advertised_host:
        interface = command_result(local_command(local, ["peer", "interface", "list", "--json"]), timeout)
        try:
            advertised_host = extract_advertised_host(interface["stdout"]) if interface["exit_code"] == 0 else None
        except SmokeError as error:
            advertised_host = None
            interface["parse_error"] = str(error)
        interface_ok = bool(advertised_host)
        compact("local-advertised-host", interface_ok, advertised_host or interface.get("parse_error", interface["stderr"] or "not found"))
        records.append({"phase": "local-advertised-host", "result": interface, "passed": interface_ok})
        all_passed &= interface_ok
    else:
        compact("local-advertised-host", True, f"override {advertised_host}")
        records.append({"phase": "local-advertised-host", "passed": True, "override": advertised_host})

    if advertised_host:
        for peer in config["peers"]:
            target = f"{local['identity']}@{local['team']}.{advertised_host}"
            remote_doctor = command_result(remote_command(peer, peer["atm_command"] + ["doctor", "--json"]), timeout)
            remote_local = {**local, "expected_daemon_version": peer["expected_daemon_version"]}
            doctor_ok, remote_doctor_detail = doctor_matches_expected(remote_local, remote_doctor)
            compact(f"{peer['name']}-doctor", doctor_ok, remote_doctor_detail)
            records.append({"phase": f"{peer['name']}-doctor", "result": remote_doctor, "passed": doctor_ok})
            all_passed &= doctor_ok
            for kind, requires_ack in (("noack", False), ("ack-required", True)):
                body = f"inbound-smoke-{peer['name']}-{kind}-{stamp}"
                send_args = ["send", target, body, "--json"]
                if requires_ack:
                    send_args.insert(-1, "--requires-ack")
                remote_send = command_result(
                    remote_command(peer, peer["atm_command"] + send_args, {"ATM_IDENTITY": peer["identity"], "ATM_TEAM": peer["team"]}),
                    timeout,
                )
                try:
                    message_id = extract_message_id(remote_send["stdout"]) if remote_send["exit_code"] == 0 else None
                except SmokeError as error:
                    message_id = None
                    remote_send["parse_error"] = str(error)
                send_ok = bool(message_id)
                compact(f"{peer['name']}-send-{kind}", send_ok, message_id or remote_send.get("parse_error", remote_send["stderr"] or "send failed"))
                records.append({"phase": f"{peer['name']}-send-{kind}", "result": remote_send, "message_id": message_id, "passed": send_ok})
                all_passed &= send_ok
                if not message_id:
                    continue
                local_read = read_received(local, message_id, timeout, time.monotonic() + receive_timeout)
                message = local_read.get("message")
                visible = message is not None
                pending = bool(message and message.get("requires_ack") is True) if requires_ack else True
                read_ok = visible and pending
                detail = message_id if read_ok else local_read.get("stderr") or "message was not visible locally"
                compact(f"{peer['name']}-read-{kind}", read_ok, detail)
                records.append({"phase": f"{peer['name']}-read-{kind}", "result": local_read, "message_id": message_id, "passed": read_ok})
                all_passed &= read_ok

    logs: dict[str, Any] = {"local": capture_log(local.get("log_command"), timeout)}
    for peer in config["peers"]:
        logs[peer["name"]] = capture_log(
            remote_command(peer, peer["log_command"]) if peer.get("log_command") else None,
            timeout,
        )
    (evidence_dir / "results.json").write_text(json.dumps({"status": "pass" if all_passed else "fail", "records": records, "logs": logs}, indent=2) + "\n", encoding="utf-8")
    write_host_panes(evidence_dir, local, config["peers"], records)
    print(f"{'PASS' if all_passed else 'FAIL'} evidence: {evidence_dir / 'results.json'}", flush=True)
    return 0 if all_passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--evidence-dir", required=True, type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=8.0)
    parser.add_argument("--receive-timeout-seconds", type=float, default=12.0)
    parser.add_argument("--host", action="store_true", help="run only this host's local checks/outbound sends")
    parser.add_argument("--handoff", action="append", type=Path, default=[], help="peer handoff.json with exact IDs for this host to verify; repeat per peer")
    args = parser.parse_args()
    config = load_config(args.config)
    return run_host(config, args.evidence_dir, args.timeout_seconds, args.receive_timeout_seconds, args.handoff) if args.host else run(config, args.evidence_dir, args.timeout_seconds, args.receive_timeout_seconds)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"inbound-peer-smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

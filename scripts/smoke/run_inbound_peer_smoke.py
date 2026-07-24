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
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time
from typing import Any


MAX_CAPTURE = 8192
SECRET = re.compile(r"(?i)(-----BEGIN[^-]+-----|(?:token|secret|password|capability|private[_-]?key)\s*[=:]\s*[^\s,]+)")


class SmokeError(RuntimeError):
    """A configuration or smoke assertion error."""


def sanitize(value: str) -> str:
    return SECRET.sub("<redacted>", value)[-MAX_CAPTURE:]


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
    if "advertised_host" in local:
        require_string(local["advertised_host"], "local.advertised_host")
    if "log_command" in local:
        require_argv(local["log_command"], "local.log_command")
    peers = value.get("peers")
    if not isinstance(peers, list) or not peers:
        raise SmokeError("peers must be a non-empty array")
    for index, peer in enumerate(peers):
        if not isinstance(peer, dict):
            raise SmokeError(f"peers[{index}] must be an object")
        require_string(peer.get("name"), f"peers[{index}].name")
        require_argv(peer.get("ssh_command"), f"peers[{index}].ssh_command")
        require_argv(peer.get("atm_command"), f"peers[{index}].atm_command")
        require_string(peer.get("identity"), f"peers[{index}].identity")
        require_string(peer.get("team"), f"peers[{index}].team")
        shell = peer.get("shell", "posix")
        if shell not in {"posix", "powershell"}:
            raise SmokeError(f"peers[{index}].shell must be posix or powershell")
        if "log_command" in peer:
            require_argv(peer["log_command"], f"peers[{index}].log_command")
    return value


def command_result(command: list[str], timeout: float) -> dict[str, Any]:
    try:
        completed = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout, check=False)
        return {"command": command, "exit_code": completed.returncode, "stdout": sanitize(completed.stdout), "stderr": sanitize(completed.stderr)}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": command, "exit_code": None, "stdout": "", "stderr": sanitize(str(error))}


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


def extract_message_id(raw: str) -> str:
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise SmokeError(f"send did not return JSON: {error}") from error

    def find(item: Any) -> str | None:
        if isinstance(item, dict):
            for key in ("message_id", "messageId"):
                if isinstance(item.get(key), str) and item[key]:
                    return item[key]
            for nested in item.values():
                found = find(nested)
                if found:
                    return found
        elif isinstance(item, list):
            for nested in item:
                found = find(nested)
                if found:
                    return found
        return None

    found = find(value)
    if not found:
        raise SmokeError("send JSON did not include a message_id")
    return found


def extract_advertised_host(raw: str) -> str:
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise SmokeError(f"peer interface list did not return JSON: {error}") from error

    def walk(item: Any) -> str | None:
        if isinstance(item, dict):
            enabled = item.get("enabled")
            host = item.get("advertise_host", item.get("advertised_host"))
            if enabled is not False and isinstance(host, str) and host:
                return host
            for nested in item.values():
                found = walk(nested)
                if found:
                    return found
        elif isinstance(item, list):
            for nested in item:
                found = walk(nested)
                if found:
                    return found
        return None

    host = walk(value)
    if not host:
        raise SmokeError("peer interface list JSON has no enabled advertise_host; set local.advertised_host")
    return host


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


def compact(label: str, passed: bool, detail: str) -> None:
    print(f"{'PASS' if passed else 'FAIL'} {label}: {detail}", flush=True)


def capture_log(command: list[str] | None, timeout: float) -> dict[str, Any] | None:
    return command_result(command, timeout) if command else None


def run(config: dict[str, Any], output_root: Path, timeout: float, receive_timeout: float) -> int:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_dir = output_root / stamp
    evidence_dir.mkdir(parents=True, exist_ok=False)
    local = config["local"]
    records: list[dict[str, Any]] = []
    all_passed = True

    doctor = command_result(local_command(local, ["doctor", "--json"]), timeout)
    local_ready = doctor["exit_code"] == 0
    compact("local-doctor", local_ready, "ready" if local_ready else doctor["stderr"] or "doctor failed")
    records.append({"phase": "local-doctor", "result": doctor, "passed": local_ready})
    all_passed &= local_ready
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
            doctor_ok = remote_doctor["exit_code"] == 0
            compact(f"{peer['name']}-doctor", doctor_ok, "ready" if doctor_ok else remote_doctor["stderr"] or "doctor failed")
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
    print(f"{'PASS' if all_passed else 'FAIL'} evidence: {evidence_dir / 'results.json'}", flush=True)
    return 0 if all_passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--evidence-dir", required=True, type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=8.0)
    parser.add_argument("--receive-timeout-seconds", type=float, default=12.0)
    args = parser.parse_args()
    return run(load_config(args.config), args.evidence_dir, args.timeout_seconds, args.receive_timeout_seconds)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"inbound-peer-smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

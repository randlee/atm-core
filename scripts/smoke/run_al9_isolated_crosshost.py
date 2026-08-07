#!/usr/bin/env python3
"""Prepare and execute an isolated AL.9 plain-TCP cross-host proof.

This runner is intentionally unable to discover, stop, or reuse an ambient
daemon.  Each configured host provides its replacement ``atm`` command and
clean-room environment explicitly. The only application commands it issues
are replacement-runtime ``atm send`` and ``atm read``. A separate preflight
command reports the binary/runtime revision before those commands can run.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys
from typing import Any


class SmokeError(RuntimeError):
    """An isolated-proof preflight or evidence assertion failed."""


def require_string(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SmokeError(f"{field} must be a non-empty string")
    return value


def require_argv(value: Any, field: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        raise SmokeError(f"{field} must be a non-empty argv array")
    return list(value)


def load_config(path: Path) -> dict[str, Any]:
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SmokeError(f"cannot read AL.9 proof config {path}: {error}") from error
    if not isinstance(config, dict) or config.get("schema_version") != 1:
        raise SmokeError("AL.9 proof config requires schema_version 1")
    for side in ("sender", "receiver"):
        entry = config.get(side)
        if not isinstance(entry, dict):
            raise SmokeError(f"{side} must be an object")
        require_argv(entry.get("atm_command"), f"{side}.atm_command")
        require_argv(entry.get("preflight_command"), f"{side}.preflight_command")
        if "ssh_command" in entry:
            require_argv(entry["ssh_command"], f"{side}.ssh_command")
        require_string(entry.get("revision"), f"{side}.revision")
        require_string(entry.get("identity"), f"{side}.identity")
        require_string(entry.get("team"), f"{side}.team")
        environment = entry.get("environment")
        if not isinstance(environment, dict) or not environment:
            raise SmokeError(f"{side}.environment must name the isolated replacement runtime")
        if not all(isinstance(key, str) and isinstance(value, str) for key, value in environment.items()):
            raise SmokeError(f"{side}.environment must contain string keys and values")
    if config["sender"]["revision"] != config["receiver"]["revision"]:
        raise SmokeError("sender and receiver must name the same replacement revision")
    require_string(config.get("recipient"), "recipient")
    return config


def command_with_environment(command: list[str], environment: dict[str, str]) -> list[str]:
    """Return argv for a direct local execution without a shell."""
    return ["env", *[f"{key}={value}" for key, value in sorted(environment.items())], *command]


def side_command(side: dict[str, Any], command: list[str]) -> list[str]:
    """Build a local or SSH argv without allowing an ambient environment."""
    environment = side["environment"]
    ssh = side.get("ssh_command")
    if ssh is None:
        return command_with_environment(command, environment)
    assignments = " ".join(
        f"{key}={shlex.quote(value)}" for key, value in sorted(environment.items())
    )
    script = f"{assignments} {shlex.join(command)}"
    return require_argv(ssh, "side.ssh_command") + ["sh", "-lc", script]


def replacement_preflight_command(side: dict[str, Any]) -> list[str]:
    return side_command(side, require_argv(side["preflight_command"], "side.preflight_command"))


def parse_replacement_preflight(output: str, expected_revision: str) -> dict[str, Any]:
    try:
        doctor = json.loads(output)
    except json.JSONDecodeError as error:
        raise SmokeError(f"replacement preflight returned invalid JSON: {error}") from error
    if not isinstance(doctor, dict):
        raise SmokeError("replacement preflight must return a JSON object")
    runtime = doctor.get("replacement_runtime")
    revision = doctor.get("revision")
    if runtime != "atm-http-runtime":
        raise SmokeError(f"preflight replacement_runtime {runtime!r} is not atm-http-runtime")
    if revision != expected_revision:
        raise SmokeError(f"preflight revision {revision!r} != expected {expected_revision!r}")
    return doctor


def send_command(sender: dict[str, Any], recipient: str, body: str) -> list[str]:
    return side_command(
        sender,
        require_argv(sender["atm_command"], "sender.atm_command")
        + ["send", "--team", sender["team"], recipient, body, "--json"],
    )


def read_command(receiver: dict[str, Any], message_id: str) -> list[str]:
    return side_command(
        receiver,
        require_argv(receiver["atm_command"], "receiver.atm_command")
        + ["read", "--team", receiver["team"], "--message-id", message_id, "--json"],
    )


def result(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True, check=False)


def require_success(label: str, completed: subprocess.CompletedProcess[str]) -> str:
    if completed.returncode:
        raise SmokeError(f"{label} failed ({completed.returncode}): {completed.stderr.strip()}")
    return completed.stdout


def message_id_from_send(output: str) -> str:
    try:
        value = json.loads(output)
    except json.JSONDecodeError as error:
        raise SmokeError(f"send returned invalid JSON: {error}") from error
    message_id = value.get("message_id") if isinstance(value, dict) else None
    if not isinstance(message_id, str) or not message_id:
        raise SmokeError("send JSON has no exact message_id")
    return message_id


def run(config: dict[str, Any], body: str) -> dict[str, Any]:
    sender = config["sender"]
    receiver = config["receiver"]
    sender_doctor = parse_replacement_preflight(
        require_success(
            "sender replacement preflight", result(replacement_preflight_command(sender))
        ),
        sender["revision"],
    )
    receiver_doctor = parse_replacement_preflight(
        require_success(
            "receiver replacement preflight", result(replacement_preflight_command(receiver))
        ),
        receiver["revision"],
    )
    sent = require_success("isolated replacement send", result(send_command(sender, config["recipient"], body)))
    message_id = message_id_from_send(sent)
    received = require_success("isolated replacement read", result(read_command(receiver, message_id)))
    return {
        "sender_revision": sender["revision"],
        "receiver_revision": receiver["revision"],
        "message_id": message_id,
        "sender_route": sender_doctor.get("route_evidence"),
        "receiver_storage": receiver_doctor.get("storage_evidence"),
        "receiver_hook": receiver_doctor.get("hook_evidence"),
        "read": received,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=Path)
    parser.add_argument("--body", required=True)
    args = parser.parse_args()
    try:
        evidence = run(load_config(args.config), args.body)
    except SmokeError as error:
        print(f"AL.9 isolated cross-host proof: FAIL: {error}", file=sys.stderr)
        return 1
    print(json.dumps(evidence, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())

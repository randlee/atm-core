"""Small shared, sanitized primitives for ATM smoke runners."""
from __future__ import annotations

import json
import re
import subprocess
from typing import Any

# Smoke control responses (notably `atm doctor --json`) include a full roster
# and can legitimately exceed 8 KiB.  Truncating a JSON response before its
# caller parses it converted a healthy daemon into a false smoke failure.
# Keep one bounded-but-complete control-plane response instead; callers still
# redact it before persisting evidence.
MAX_CAPTURE = 1_048_576
SECRET = re.compile(r"(?i)(-----BEGIN[^-]+-----|(?:token|secret|password|capability|private[_-]?key)\s*[=:]\s*[^\s,]+)")


class SmokeError(RuntimeError):
    """A smoke prerequisite or assertion failed."""


def sanitize(value: str) -> str:
    return SECRET.sub("<redacted>", value)[-MAX_CAPTURE:]


def command_result(command: list[str], timeout: float = 15.0) -> dict[str, Any]:
    """Run one command and retain only bounded, redacted evidence."""
    try:
        completed = subprocess.run(
            command, capture_output=True, text=True, encoding="utf-8",
            errors="replace", timeout=timeout, check=False,
        )
        return {
            "command": command,
            "exit_code": completed.returncode,
            "stdout": sanitize(completed.stdout),
            "stderr": sanitize(completed.stderr),
        }
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": command, "exit_code": None, "stdout": "", "stderr": sanitize(str(error))}


def message_id_from_value(value: Any) -> str:
    if isinstance(value, dict):
        for key in ("message_id", "messageId"):
            if isinstance(value.get(key), str) and value[key]:
                return value[key]
        for child in value.values():
            try:
                return message_id_from_value(child)
            except SmokeError:
                continue
    if isinstance(value, list):
        for child in value:
            try:
                return message_id_from_value(child)
            except SmokeError:
                continue
    raise SmokeError("ATM JSON did not contain a message ID")


def extract_message_id(raw: str) -> str:
    try:
        return message_id_from_value(json.loads(raw))
    except json.JSONDecodeError as error:
        raise SmokeError(f"send did not return JSON: {error}") from error


def advertised_host_from_value(interfaces: Any) -> str:
    stack = [interfaces]
    while stack:
        value = stack.pop()
        if isinstance(value, dict):
            host = value.get("advertise_host", value.get("advertised_host"))
            if value.get("enabled") is not False and isinstance(host, str) and host:
                return host
            stack.extend(value.values())
        elif isinstance(value, list):
            stack.extend(value)
    raise SmokeError("peer interface list JSON has no enabled advertise_host; set local.advertised_host")


def extract_advertised_host(raw: str) -> str:
    try:
        return advertised_host_from_value(json.loads(raw))
    except json.JSONDecodeError as error:
        raise SmokeError(f"peer interface list did not return JSON: {error}") from error

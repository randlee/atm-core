"""Strict, credential-safe validation for channel-worker result envelopes."""
from __future__ import annotations

import json
import re
from typing import Any

REQUIRED_FIELDS = {
    "channel", "status", "tag", "commit", "command", "exit_status",
    "error", "attempts", "workflow_url", "job_url", "evidence",
    "registry_outcome", "verification", "sanitized_diagnostic", "checks", "required_checks",
}
STATUSES = {"passed", "failed", "blocked", "apparently_available", "taken", "indeterminate"}
_FENCE = re.compile(r"```(?:json)?\s*\n?(.*?)\n?```", re.DOTALL | re.IGNORECASE)


_SECRET_KEY = re.compile(r"(?i)(?:authorization|(?:.*[_-])?(?:token|password|secret|api[_-]?key)|_auth)")
_SECRET_JSON = re.compile(
    r'''(?i)(["'](?:authorization|(?:[\w-]*[_-])?(?:token|password|secret|api[_-]?key)|_auth)["']\s*:\s*)["'](?:\\.|[^"'\\])*["']'''
)
_AUTH_HEADER = re.compile(r"(?i)authorization\s*[:=]\s*(?:(?:basic|bearer|token)\s+)?[^\s,;]+")
_SECRET_ASSIGNMENT = re.compile(r"(?i)\b((?:[\w-]*[_-])?(?:token|password|secret|api[_-]?key)|_auth)\s*[:=]\s*[^\s,;]+")
_BEARER = re.compile(r"(?i)\bbearer\s+[^\s,;]+")
_SECRET_ARG = re.compile(r"(?i)(--(?:[\w-]*-)?(?:token|password|secret|api-key)\s+)[^\s,;]+")


def redact_diagnostic(text: str) -> str:
    """Redact credential syntax while preserving useful non-secret diagnostics."""
    text = _SECRET_JSON.sub(r'\1"<redacted>"', text)
    text = _AUTH_HEADER.sub("Authorization=<redacted>", text)
    text = _SECRET_ASSIGNMENT.sub(r"\1=<redacted>", text)
    text = _BEARER.sub("Bearer <redacted>", text)
    return _SECRET_ARG.sub(r"\1<redacted>", text)


def redact_result(value: Any) -> Any:
    """Copy and sanitize every nested report field before returning it to callers."""
    if isinstance(value, dict):
        return {key: "<redacted>" if isinstance(key, str) and _SECRET_KEY.fullmatch(key)
                else redact_result(item) for key, item in value.items()}
    if isinstance(value, list):
        sanitized = []
        hide_next = False
        for item in value:
            sanitized.append("<redacted>" if hide_next else redact_result(item))
            hide_next = isinstance(item, str) and item.startswith("--") and bool(_SECRET_KEY.fullmatch(item[2:]))
        return sanitized
    if isinstance(value, str):
        return redact_diagnostic(value)
    return value


class WorkerResultError(ValueError):
    """A missing, malformed, incomplete, or inconsistent worker result."""


def parse_fenced_result(text: str) -> dict[str, Any]:
    matches = _FENCE.findall(text)
    if len(matches) != 1:
        raise WorkerResultError("worker result must contain exactly one fenced JSON object")
    try:
        result = json.loads(matches[0])
    except json.JSONDecodeError as error:
        raise WorkerResultError(f"worker result contains invalid JSON: {error.msg}") from None
    if not isinstance(result, dict):
        raise WorkerResultError("worker result JSON must be an object")
    return validate_result(result)


def validate_result(result: dict[str, Any]) -> dict[str, Any]:
    result = redact_result(result)
    missing = sorted(REQUIRED_FIELDS - result.keys())
    if missing:
        raise WorkerResultError("worker result missing required fields: " + ", ".join(missing))
    if not isinstance(result["channel"], str) or not result["channel"]:
        raise WorkerResultError("worker result channel must be a non-empty string")
    if not isinstance(result["status"], str) or result["status"] not in STATUSES:
        raise WorkerResultError("worker result has an invalid status")
    if not isinstance(result["tag"], str) or not result["tag"]:
        raise WorkerResultError("worker result tag must be present")
    if not isinstance(result["commit"], str) or not result["commit"]:
        raise WorkerResultError("worker result commit must be present")
    if not isinstance(result["command"], list) or not result["command"] or not all(isinstance(item, str) and item for item in result["command"]):
        raise WorkerResultError("worker result command must be a non-empty argv array")
    if not isinstance(result["exit_status"], int):
        raise WorkerResultError("worker result exit_status must be an integer")
    if result["status"] == "passed" and result["exit_status"] != 0:
        raise WorkerResultError("successful worker result must have exit_status 0")
    if not isinstance(result["attempts"], int) or result["attempts"] < 1:
        raise WorkerResultError("worker result attempts must be a positive integer")
    for field in ("workflow_url", "job_url"):
        if not isinstance(result[field], str) or not result[field]:
            raise WorkerResultError(f"worker result {field} must be a non-empty string")
    for field in ("checks", "required_checks"):
        if not isinstance(result[field], list):
            raise WorkerResultError(f"worker result {field} must be an array")
    if any(
        not isinstance(entry, dict) or not isinstance(entry.get("kind"), str) or not entry["kind"]
        or not isinstance(entry.get("status"), str) or entry["status"] not in {"passed", "failed", "blocked"}
        for entry in result["checks"]
    ):
        raise WorkerResultError("worker result checks must contain kind and valid status")
    if any(
        not isinstance(entry, dict) or not isinstance(entry.get("kind"), str) or not entry["kind"]
        or not isinstance(entry.get("reason"), str) or not entry["reason"]
        for entry in result["required_checks"]
    ):
        raise WorkerResultError("worker result required_checks must contain kind and reason")
    if result["status"] == "passed" and not result["checks"]:
        raise WorkerResultError("successful worker result must preserve observed checks")
    if result["status"] == "passed" and any(entry["status"] != "passed" for entry in result["checks"]):
        raise WorkerResultError("successful worker result cannot contain failed checks")
    if result["status"] == "passed" and result["required_checks"]:
        raise WorkerResultError("successful worker result cannot retain outstanding required_checks")
    for field in ("evidence", "registry_outcome"):
        if not isinstance(result[field], (str, list, dict)):
            raise WorkerResultError(f"worker result {field} must be structured or text")
    if not isinstance(result["verification"], (str, list, dict)):
        raise WorkerResultError("worker result verification must be structured or text")
    if not isinstance(result["sanitized_diagnostic"], str):
        raise WorkerResultError("worker result sanitized_diagnostic must be text")
    if result["status"] == "passed" and result["error"] not in (None, "", {}):
        raise WorkerResultError("successful worker result must have error=null or empty")
    if result["status"] not in {"passed", "apparently_available", "taken"} and not result["error"]:
        raise WorkerResultError("failed or blocked worker result must preserve error details")
    return result


def aggregate_results(results: list[dict[str, Any] | None], expected_channels: list[str]) -> list[dict[str, Any]]:
    """Validate every result, retaining valid channels and recording contract failures."""
    validated = []
    for channel, result in zip(expected_channels, results):
        if result is None:
            item = _contract_failure(channel, "missing worker result")
        elif not isinstance(result, dict):
            item = _contract_failure(channel, "worker result must be a JSON object")
        else:
            try:
                item = validate_result(result)
                if item["channel"] != channel:
                    raise WorkerResultError(f"worker result channel mismatch: expected {channel}")
            except WorkerResultError as error:
                item = _contract_failure(channel, str(error))
        validated.append(item)
    for channel in expected_channels[len(results):]:
        validated.append(_contract_failure(channel, "missing worker result"))
    return validated


def _contract_failure(channel: str, detail: str) -> dict[str, Any]:
    return {
        "channel": channel, "status": "failed", "tag": "unavailable", "commit": "unavailable",
        "command": ["worker-result-validation"], "exit_status": -1, "error": {"code": "REPORTING.CONTRACT_FAILURE", "message": detail},
        "attempts": 1, "workflow_url": "unavailable", "job_url": "unavailable",
        "evidence": "worker response envelope", "registry_outcome": "unavailable",
        "verification": [], "sanitized_diagnostic": detail,
        "checks": [{"kind": "worker_result_contract", "status": "failed"}], "required_checks": [],
    }

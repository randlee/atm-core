#!/usr/bin/env python3
"""Trusted agent resolution and strict invocation/result contracts.

This module is intentionally data-only: it parses registries and frontmatter but
never imports or executes an agent definition.  A caller resolves once, passes
the returned immutable descriptor to the harness Agent tool, validates the
single fenced response, then writes a credential-safe attestation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
from typing import Any


SCHEMA_VERSION = "1.0.0"
AGENT_MODELS = {
    "anchor-plan-review": {"claude": "sonnet", "codex": "luna"},
    "anchor-sprint-review": {"claude": "sonnet", "codex": "luna"},
    "anchor-tuning": {"claude": "opus", "codex": "sol"},
}
OPERATIONS = {"plan_review", "sprint_review", "testability", "tune", "triage"}
CORRELATION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
FENCE_RE = re.compile(r"```json[ \t]*\r?\n(.*?)\r?\n```", re.DOTALL)


class RunnerContractError(ValueError):
    """A typed, safe-to-display contract failure."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


def _fail(code: str, message: str) -> None:
    raise RunnerContractError(code, message)


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _regular_file(path: Path) -> bool:
    try:
        mode = path.stat().st_mode
    except OSError:
        return False
    return stat.S_ISREG(mode)


def _confined(root: Path, candidate: Path) -> bool:
    try:
        candidate.relative_to(root)
    except ValueError:
        return False
    return True


def _frontmatter(text: str) -> dict[str, Any]:
    if not text.startswith("---\n"):
        _fail("RUNTIME.FRONTMATTER", "agent definition lacks YAML frontmatter")
    end = text.find("\n---\n", 4)
    if end < 0:
        _fail("RUNTIME.FRONTMATTER", "agent definition has unterminated YAML frontmatter")
    value: dict[str, str] = {}
    for line in text[4:end].splitlines():
        if not line.strip() or line.lstrip().startswith("#") or line.startswith((" ", "\t")):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_-]*):[ \t]*(.*)", line)
        if not match:
            _fail("RUNTIME.FRONTMATTER", "agent frontmatter is malformed")
        key, raw = match.groups()
        if key in value:
            _fail("RUNTIME.FRONTMATTER", "agent frontmatter has a duplicate key")
        value[key] = raw.strip().strip('"\'')
    return value


def _registry_agent_entry(text: str, agent: str) -> dict[str, str]:
    """Parse the deliberately small registry subset without a YAML dependency."""

    in_agents = False
    current: str | None = None
    entries: dict[str, dict[str, str]] = {}
    for raw_line in text.splitlines():
        if not raw_line.strip() or raw_line.lstrip().startswith("#"):
            continue
        if re.fullmatch(r"agents:[ \t]*", raw_line):
            in_agents = True
            current = None
            continue
        if re.match(r"^[A-Za-z_]", raw_line):
            if in_agents:
                break
            continue
        if not in_agents:
            continue
        match_agent = re.fullmatch(r"  ([A-Za-z0-9_-]+):[ \t]*", raw_line)
        if match_agent:
            current = match_agent.group(1)
            if current in entries:
                _fail("RUNTIME.REGISTRY", "agent registry has a duplicate identity")
            entries[current] = {}
            continue
        match_field = re.fullmatch(r"    ([a-z_]+):[ \t]*(.*)", raw_line)
        if match_field and current:
            key, value = match_field.groups()
            if key in entries[current]:
                _fail("RUNTIME.REGISTRY", "agent registry entry has a duplicate field")
            entries[current][key] = value.strip().strip('"\'')
            continue
        _fail("RUNTIME.REGISTRY", "agent registry is malformed")
    try:
        return entries[agent]
    except KeyError:
        _fail("RUNTIME.REGISTRY", "agent registry lacks the requested agent")


def resolve_agent(plugin_root: str | Path, harness: str, agent: str) -> dict[str, str]:
    """Resolve and validate one registry entry without executing it."""

    if harness not in {"claude", "codex"}:
        _fail("RUNTIME.HARNESS", "harness must be claude or codex")
    if agent not in AGENT_MODELS:
        _fail("RUNTIME.IDENTITY", "agent is not registered for plan-anchor")

    try:
        root = Path(plugin_root).resolve(strict=True)
    except OSError:
        _fail("RUNTIME.PATH", "plugin root does not resolve")
    registry = root / f".{harness}" / "agents" / "registry.yaml"
    try:
        resolved_registry = registry.resolve(strict=True)
    except OSError:
        _fail("RUNTIME.REGISTRY", "agent registry is missing or not a regular file")
    if registry.is_symlink() or not _confined(root, resolved_registry) or not _regular_file(resolved_registry):
        _fail("RUNTIME.REGISTRY", "agent registry is missing or not a regular file")
    try:
        entry = _registry_agent_entry(registry.read_text(encoding="utf-8"), agent)
    except (OSError, UnicodeError):
        _fail("RUNTIME.REGISTRY", "agent registry is malformed or lacks the requested agent")
    if not isinstance(entry, dict) or set(entry) != {"version", "path"}:
        _fail("RUNTIME.REGISTRY", "agent registry entry has an invalid shape")
    if not isinstance(entry["version"], str) or not isinstance(entry["path"], str):
        _fail("RUNTIME.REGISTRY", "agent registry version and path must be strings")

    lexical = root / entry["path"]
    try:
        target = lexical.resolve(strict=True)
    except OSError:
        _fail("RUNTIME.PATH", "registered agent path does not resolve")
    if not _confined(root, target) or not _regular_file(target):
        _fail("RUNTIME.PATH", "registered agent path is untrusted")
    if lexical.is_symlink():
        _fail("RUNTIME.PATH", "registered agent path must not be a symlink")

    try:
        content = target.read_bytes()
        text = content.decode("utf-8")
    except (OSError, UnicodeError):
        _fail("RUNTIME.PATH", "registered agent definition is not readable UTF-8")
    frontmatter = _frontmatter(text)
    if frontmatter.get("name") != agent:
        _fail("RUNTIME.IDENTITY", "agent frontmatter identity differs from registry")
    if frontmatter.get("version") != entry["version"]:
        _fail("RUNTIME.VERSION", "agent frontmatter version differs from registry")
    expected_model = AGENT_MODELS[agent][harness]
    if frontmatter.get("model") != expected_model:
        _fail("RUNTIME.MODEL", "agent frontmatter model is invalid for its harness and role")

    return {
        "agent": agent,
        "harness": harness,
        "path": target.relative_to(root).as_posix(),
        "version": entry["version"],
        "model": expected_model,
        "sha256": sha256_bytes(content),
    }


def verify_resolved_agent(plugin_root: str | Path, runtime: Any) -> dict[str, str]:
    """Detect definition changes between resolution and invocation."""

    validate_runtime(runtime)
    current = resolve_agent(plugin_root, runtime["harness"], runtime["agent"])
    if current["path"] != runtime["path"]:
        _fail("RUNTIME.PATH", "registered agent path changed after resolution")
    if current["version"] != runtime["version"]:
        _fail("RUNTIME.VERSION", "registered agent version changed after resolution")
    if current["model"] != runtime["model"]:
        _fail("RUNTIME.MODEL", "registered agent model changed after resolution")
    if current["sha256"] != runtime["sha256"]:
        _fail("RUNTIME.HASH", "registered agent content changed after resolution")
    return current


def validate_runtime(runtime: Any) -> None:
    if not isinstance(runtime, dict) or set(runtime) != {
        "agent", "harness", "path", "version", "model", "sha256"
    }:
        _fail("ENVELOPE.RUNTIME", "runtime descriptor has an invalid shape")
    if runtime.get("agent") not in AGENT_MODELS:
        _fail("ENVELOPE.RUNTIME", "runtime agent is unknown")
    harness = runtime.get("harness")
    if harness not in {"claude", "codex"}:
        _fail("ENVELOPE.RUNTIME", "runtime harness is invalid")
    if runtime.get("model") != AGENT_MODELS[runtime["agent"]][harness]:
        _fail("ENVELOPE.RUNTIME", "runtime model is invalid for its harness and role")
    if not isinstance(runtime.get("path"), str) or Path(runtime["path"]).is_absolute():
        _fail("ENVELOPE.RUNTIME", "runtime path must be registry-relative")
    if ".." in Path(runtime["path"]).parts:
        _fail("ENVELOPE.RUNTIME", "runtime path must not traverse its plugin root")
    if not isinstance(runtime.get("version"), str) or not runtime["version"]:
        _fail("ENVELOPE.RUNTIME", "runtime version is invalid")
    if not isinstance(runtime.get("sha256"), str) or not SHA256_RE.fullmatch(runtime["sha256"]):
        _fail("ENVELOPE.RUNTIME", "runtime sha256 is invalid")


def validate_request(request: Any) -> dict[str, Any]:
    if not isinstance(request, dict) or set(request) != {
        "schema_version", "runtime", "operation", "correlation_id", "params"
    }:
        _fail("ENVELOPE.REQUEST", "request must contain only the required top-level fields")
    if request["schema_version"] != SCHEMA_VERSION:
        _fail("ENVELOPE.VERSION", "request schema_version is unsupported")
    validate_runtime(request["runtime"])
    if request["operation"] not in OPERATIONS:
        _fail("ENVELOPE.OPERATION", "request operation is unsupported")
    if not isinstance(request["correlation_id"], str) or not CORRELATION_RE.fullmatch(request["correlation_id"]):
        _fail("ENVELOPE.CORRELATION", "request correlation_id is invalid")
    if not isinstance(request["params"], dict):
        _fail("ENVELOPE.PARAMS", "request params must be an object")
    return request


def _reject_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, child in pairs:
        if key in value:
            _fail("ENVELOPE.DUPLICATE_KEY", "JSON object contains a duplicate key")
        value[key] = child
    return value


def parse_single_fenced_json(text: str, *, kind: str) -> dict[str, Any]:
    blocks = FENCE_RE.findall(text)
    remainder = FENCE_RE.sub("", text).strip()
    if len(blocks) != 1 or remainder:
        _fail(f"ENVELOPE.{kind.upper()}_FENCE", f"{kind} must be exactly one fenced JSON object")
    try:
        value = json.loads(blocks[0], object_pairs_hook=_reject_duplicate_pairs)
    except json.JSONDecodeError:
        _fail(f"ENVELOPE.{kind.upper()}_JSON", f"{kind} JSON is malformed")
    if not isinstance(value, dict):
        _fail(f"ENVELOPE.{kind.upper()}_JSON", f"{kind} JSON must be an object")
    return value


def parse_request(text: str) -> dict[str, Any]:
    return validate_request(parse_single_fenced_json(text, kind="request"))


def validate_result(result: Any, correlation_id: str) -> dict[str, Any]:
    if not isinstance(result, dict) or set(result) != {
        "schema_version", "success", "canceled", "aborted_by", "data", "error", "metadata"
    }:
        _fail("ENVELOPE.RESULT", "result must contain only the required top-level fields")
    if result["schema_version"] != SCHEMA_VERSION:
        _fail("ENVELOPE.VERSION", "result schema_version is unsupported")
    for field in ("success", "canceled"):
        if not isinstance(result[field], bool):
            _fail("ENVELOPE.RESULT", f"result {field} must be boolean")
    if result["aborted_by"] is not None and not isinstance(result["aborted_by"], str):
        _fail("ENVELOPE.RESULT", "result aborted_by must be null or a string")
    metadata = result["metadata"]
    if not isinstance(metadata, dict) or metadata.get("correlation_id") != correlation_id:
        _fail("ENVELOPE.CORRELATION", "result correlation_id does not match request")
    if result["success"]:
        if not isinstance(result["data"], dict) or result["error"] is not None:
            _fail("ENVELOPE.RESULT", "successful result requires object data and null error")
    else:
        error = result["error"]
        required = {"code", "message", "recoverable", "suggested_action"}
        if result["data"] is not None or not isinstance(error, dict) or set(error) != required:
            _fail("ENVELOPE.RESULT", "failed result requires null data and a typed error")
        if not isinstance(error["code"], str) or not isinstance(error["message"], str):
            _fail("ENVELOPE.RESULT", "result error code and message must be strings")
        if not isinstance(error["recoverable"], bool):
            _fail("ENVELOPE.RESULT", "result recoverable must be boolean")
        if error["suggested_action"] is not None and not isinstance(error["suggested_action"], str):
            _fail("ENVELOPE.RESULT", "result suggested_action must be null or a string")
    return result


def parse_result(text: str, correlation_id: str) -> dict[str, Any]:
    return validate_result(parse_single_fenced_json(text, kind="result"), correlation_id)


def build_attestation(
    runtime: dict[str, Any], operation: str, correlation_id: str, *,
    started_at_ms: int, finished_at_ms: int, outcome: str,
    source_sha256: str | None = None, error_code: str | None = None,
) -> dict[str, Any]:
    validate_runtime(runtime)
    if operation not in OPERATIONS:
        _fail("ATTESTATION.OPERATION", "attestation operation is unsupported")
    if not CORRELATION_RE.fullmatch(correlation_id):
        _fail("ATTESTATION.CORRELATION", "attestation correlation_id is invalid")
    if outcome not in {"success", "failure", "timeout", "canceled"}:
        _fail("ATTESTATION.OUTCOME", "attestation outcome is invalid")
    if not isinstance(started_at_ms, int) or not isinstance(finished_at_ms, int) or finished_at_ms < started_at_ms:
        _fail("ATTESTATION.TIMING", "attestation timing is invalid")
    if source_sha256 is not None and not SHA256_RE.fullmatch(source_sha256):
        _fail("ATTESTATION.SOURCE", "attestation source digest is invalid")
    if error_code is not None and not re.fullmatch(r"[A-Z][A-Z0-9_.-]{0,63}", error_code):
        _fail("ATTESTATION.ERROR", "attestation error code is invalid")
    return {
        "schema_version": SCHEMA_VERSION,
        "runtime": runtime,
        "operation": operation,
        "correlation_id": correlation_id,
        "started_at_ms": started_at_ms,
        "finished_at_ms": finished_at_ms,
        "duration_ms": finished_at_ms - started_at_ms,
        "outcome": outcome,
        "source_sha256": source_sha256,
        "error_code": error_code,
    }


def atomic_write_json(path: str | Path, value: dict[str, Any]) -> None:
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{destination.name}.", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(canonical_json(value) + b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, destination)
    except OSError:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        _fail("ATTESTATION.WRITE", "atomic attestation write failed")


def _emit(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    resolve = sub.add_parser("resolve")
    resolve.add_argument("--plugin-root", required=True)
    resolve.add_argument("--harness", required=True, choices=("claude", "codex"))
    resolve.add_argument("--agent", required=True, choices=tuple(AGENT_MODELS))
    check = sub.add_parser("validate-fence")
    check.add_argument("--kind", required=True, choices=("request", "result"))
    check.add_argument("--file", required=True)
    check.add_argument("--correlation-id")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "resolve":
            _emit({"success": True, "data": resolve_agent(args.plugin_root, args.harness, args.agent), "error": None})
        else:
            text = Path(args.file).read_text(encoding="utf-8")
            if args.kind == "request":
                value = parse_request(text)
            else:
                if not args.correlation_id:
                    _fail("ENVELOPE.CORRELATION", "result validation requires correlation_id")
                value = parse_result(text, args.correlation_id)
            _emit({"success": True, "data": value, "error": None})
        return 0
    except RunnerContractError as exc:
        _emit({"success": False, "data": None, "error": {"code": exc.code, "message": exc.message, "recoverable": False, "suggested_action": None}})
        return 2
    except (OSError, UnicodeError):
        _emit({"success": False, "data": None, "error": {"code": "SCRIPT.IO", "message": "input could not be read", "recoverable": False, "suggested_action": None}})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

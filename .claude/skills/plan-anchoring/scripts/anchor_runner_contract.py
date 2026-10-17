#!/usr/bin/env python3
"""Trusted agent resolution and strict invocation/result contracts.

This module is intentionally data-only: it parses registries and frontmatter but
never imports or executes an agent definition.  A caller resolves once, passes
the returned immutable descriptor to the harness Agent tool (Cursor: Task), validates the
single fenced response, then writes a credential-safe attestation.

Expected contract failures are returned as ``Err(ToolError)`` values (see
``result.py``) rather than raised; only ``main`` renders them, and it alone
still catches unreadable input as ``SCRIPT.IO``.
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
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from result import Err, Ok, Result, ToolError, err  # noqa: E402


SCHEMA_VERSION = "1.0.0"
HARNESSES = ("claude", "codex", "cursor")
AGENT_MODELS = {
    "anchor-plan-review": {"claude": "sonnet", "codex": "luna", "cursor": "composer"},
    "anchor-sprint-review": {"claude": "sonnet", "codex": "luna", "cursor": "composer"},
    "anchor-tuning": {"claude": "opus", "codex": "sol", "cursor": "grok[effort=high]"},
}
OPERATIONS = {"plan_review", "sprint_review", "testability", "tune", "triage"}
CORRELATION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
FENCE_RE = re.compile(r"```json[ \t]*\r?\n(.*?)\r?\n```", re.DOTALL)


class _DuplicateKey(Exception):
    """Raised only inside the json object_pairs_hook, which cannot return a Result."""


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


def _frontmatter(text: str) -> Result[dict[str, Any], ToolError]:
    # Git may materialize checked-in Markdown with CRLF on Windows. Parse the
    # frontmatter by logical lines while retaining the raw bytes for the
    # runtime attestation hash.
    lines = text.splitlines()
    if not lines or lines[0] != "---":
        return err("RUNTIME.FRONTMATTER", "agent definition lacks YAML frontmatter")
    try:
        end = lines.index("---", 1)
    except ValueError:
        return err("RUNTIME.FRONTMATTER", "agent definition has unterminated YAML frontmatter")
    value: dict[str, str] = {}
    for line in lines[1:end]:
        if not line.strip() or line.lstrip().startswith("#") or line.startswith((" ", "\t")):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_-]*):[ \t]*(.*)", line)
        if not match:
            return err("RUNTIME.FRONTMATTER", "agent frontmatter is malformed")
        key, raw = match.groups()
        if key in value:
            return err("RUNTIME.FRONTMATTER", "agent frontmatter has a duplicate key")
        value[key] = raw.strip().strip('"\'')
    return Ok(value)


def _registry_agent_entry(text: str, agent: str) -> Result[dict[str, str], ToolError]:
    """Parse the deliberately small registry subset without a YAML dependency."""

    in_agents = False
    current: Optional[str] = None
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
                return err("RUNTIME.REGISTRY", "agent registry has a duplicate identity")
            entries[current] = {}
            continue
        match_field = re.fullmatch(r"    ([a-z_]+):[ \t]*(.*)", raw_line)
        if match_field and current:
            key, value = match_field.groups()
            if key in entries[current]:
                return err("RUNTIME.REGISTRY", "agent registry entry has a duplicate field")
            entries[current][key] = value.strip().strip('"\'')
            continue
        return err("RUNTIME.REGISTRY", "agent registry is malformed")
    if agent not in entries:
        return err("RUNTIME.REGISTRY", "agent registry lacks the requested agent")
    return Ok(entries[agent])


def resolve_agent(plugin_root: str | Path, harness: str, agent: str) -> Result[dict[str, str], ToolError]:
    """Resolve and validate one registry entry without executing it."""

    if harness not in HARNESSES:
        return err("RUNTIME.HARNESS", "harness must be claude, codex, or cursor")
    if agent not in AGENT_MODELS:
        return err("RUNTIME.IDENTITY", "agent is not registered for plan-anchor")

    try:
        root = Path(plugin_root).resolve(strict=True)
    except OSError:
        return err("RUNTIME.PATH", "plugin root does not resolve")
    registry = root / f".{harness}" / "agents" / "registry.yaml"
    try:
        resolved_registry = registry.resolve(strict=True)
    except OSError:
        return err("RUNTIME.REGISTRY", "agent registry is missing or not a regular file")
    if registry.is_symlink() or not _confined(root, resolved_registry) or not _regular_file(resolved_registry):
        return err("RUNTIME.REGISTRY", "agent registry is missing or not a regular file")
    try:
        registry_text = registry.read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return err("RUNTIME.REGISTRY", "agent registry is malformed or lacks the requested agent")
    parsed = _registry_agent_entry(registry_text, agent)
    if isinstance(parsed, Err):
        return parsed
    entry = parsed.value
    if not isinstance(entry, dict) or set(entry) != {"version", "path"}:
        return err("RUNTIME.REGISTRY", "agent registry entry has an invalid shape")
    if not isinstance(entry["version"], str) or not isinstance(entry["path"], str):
        return err("RUNTIME.REGISTRY", "agent registry version and path must be strings")

    lexical = root / entry["path"]
    try:
        target = lexical.resolve(strict=True)
    except OSError:
        return err("RUNTIME.PATH", "registered agent path does not resolve")
    if not _confined(root, target) or not _regular_file(target):
        return err("RUNTIME.PATH", "registered agent path is untrusted")
    if lexical.is_symlink():
        return err("RUNTIME.PATH", "registered agent path must not be a symlink")

    try:
        content = target.read_bytes()
        text = content.decode("utf-8")
    except (OSError, UnicodeError):
        return err("RUNTIME.PATH", "registered agent definition is not readable UTF-8")
    parsed_frontmatter = _frontmatter(text)
    if isinstance(parsed_frontmatter, Err):
        return parsed_frontmatter
    frontmatter = parsed_frontmatter.value
    if frontmatter.get("name") != agent:
        return err("RUNTIME.IDENTITY", "agent frontmatter identity differs from registry")
    if frontmatter.get("version") != entry["version"]:
        return err("RUNTIME.VERSION", "agent frontmatter version differs from registry")
    expected_model = AGENT_MODELS[agent][harness]
    if frontmatter.get("model") != expected_model:
        return err("RUNTIME.MODEL", "agent frontmatter model is invalid for its harness and role")

    return Ok({
        "agent": agent,
        "harness": harness,
        "path": target.relative_to(root).as_posix(),
        "version": entry["version"],
        "model": expected_model,
        "sha256": sha256_bytes(content),
    })


def verify_resolved_agent(plugin_root: str | Path, runtime: Any) -> Result[dict[str, str], ToolError]:
    """Detect definition changes between resolution and invocation."""

    checked = validate_runtime(runtime)
    if isinstance(checked, Err):
        return checked
    resolved = resolve_agent(plugin_root, runtime["harness"], runtime["agent"])
    if isinstance(resolved, Err):
        return resolved
    current = resolved.value
    if current["path"] != runtime["path"]:
        return err("RUNTIME.PATH", "registered agent path changed after resolution")
    if current["version"] != runtime["version"]:
        return err("RUNTIME.VERSION", "registered agent version changed after resolution")
    if current["model"] != runtime["model"]:
        return err("RUNTIME.MODEL", "registered agent model changed after resolution")
    if current["sha256"] != runtime["sha256"]:
        return err("RUNTIME.HASH", "registered agent content changed after resolution")
    return Ok(current)


def validate_runtime(runtime: Any) -> Result[None, ToolError]:
    if not isinstance(runtime, dict) or set(runtime) != {
        "agent", "harness", "path", "version", "model", "sha256"
    }:
        return err("ENVELOPE.RUNTIME", "runtime descriptor has an invalid shape")
    if runtime.get("agent") not in AGENT_MODELS:
        return err("ENVELOPE.RUNTIME", "runtime agent is unknown")
    harness = runtime.get("harness")
    if harness not in HARNESSES:
        return err("ENVELOPE.RUNTIME", "runtime harness is invalid")
    if runtime.get("model") != AGENT_MODELS[runtime["agent"]][harness]:
        return err("ENVELOPE.RUNTIME", "runtime model is invalid for its harness and role")
    if not isinstance(runtime.get("path"), str) or Path(runtime["path"]).is_absolute():
        return err("ENVELOPE.RUNTIME", "runtime path must be registry-relative")
    if ".." in Path(runtime["path"]).parts:
        return err("ENVELOPE.RUNTIME", "runtime path must not traverse its plugin root")
    if not isinstance(runtime.get("version"), str) or not runtime["version"]:
        return err("ENVELOPE.RUNTIME", "runtime version is invalid")
    if not isinstance(runtime.get("sha256"), str) or not SHA256_RE.fullmatch(runtime["sha256"]):
        return err("ENVELOPE.RUNTIME", "runtime sha256 is invalid")
    return Ok(None)


def validate_request(request: Any) -> Result[dict[str, Any], ToolError]:
    if not isinstance(request, dict) or set(request) != {
        "schema_version", "runtime", "operation", "correlation_id", "params"
    }:
        return err("ENVELOPE.REQUEST", "request must contain only the required top-level fields")
    if request["schema_version"] != SCHEMA_VERSION:
        return err("ENVELOPE.VERSION", "request schema_version is unsupported")
    checked = validate_runtime(request["runtime"])
    if isinstance(checked, Err):
        return checked
    if request["operation"] not in OPERATIONS:
        return err("ENVELOPE.OPERATION", "request operation is unsupported")
    if not isinstance(request["correlation_id"], str) or not CORRELATION_RE.fullmatch(request["correlation_id"]):
        return err("ENVELOPE.CORRELATION", "request correlation_id is invalid")
    if not isinstance(request["params"], dict):
        return err("ENVELOPE.PARAMS", "request params must be an object")
    return Ok(request)


def _reject_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, child in pairs:
        if key in value:
            raise _DuplicateKey()
        value[key] = child
    return value


def parse_single_fenced_json(text: str, *, kind: str) -> Result[dict[str, Any], ToolError]:
    blocks = FENCE_RE.findall(text)
    remainder = FENCE_RE.sub("", text).strip()
    if len(blocks) != 1 or remainder:
        return err(f"ENVELOPE.{kind.upper()}_FENCE", f"{kind} must be exactly one fenced JSON object")
    try:
        value = json.loads(blocks[0], object_pairs_hook=_reject_duplicate_pairs)
    except _DuplicateKey:
        return err("ENVELOPE.DUPLICATE_KEY", "JSON object contains a duplicate key")
    except json.JSONDecodeError:
        return err(f"ENVELOPE.{kind.upper()}_JSON", f"{kind} JSON is malformed")
    if not isinstance(value, dict):
        return err(f"ENVELOPE.{kind.upper()}_JSON", f"{kind} JSON must be an object")
    return Ok(value)


def parse_request(text: str) -> Result[dict[str, Any], ToolError]:
    parsed = parse_single_fenced_json(text, kind="request")
    if isinstance(parsed, Err):
        return parsed
    return validate_request(parsed.value)


def validate_result(result: Any, correlation_id: str) -> Result[dict[str, Any], ToolError]:
    if not isinstance(result, dict) or set(result) != {
        "schema_version", "success", "canceled", "aborted_by", "data", "error", "metadata"
    }:
        return err("ENVELOPE.RESULT", "result must contain only the required top-level fields")
    if result["schema_version"] != SCHEMA_VERSION:
        return err("ENVELOPE.VERSION", "result schema_version is unsupported")
    for field in ("success", "canceled"):
        if not isinstance(result[field], bool):
            return err("ENVELOPE.RESULT", f"result {field} must be boolean")
    if result["aborted_by"] is not None and not isinstance(result["aborted_by"], str):
        return err("ENVELOPE.RESULT", "result aborted_by must be null or a string")
    metadata = result["metadata"]
    if not isinstance(metadata, dict) or metadata.get("correlation_id") != correlation_id:
        return err("ENVELOPE.CORRELATION", "result correlation_id does not match request")
    if result["success"]:
        if not isinstance(result["data"], dict) or result["error"] is not None:
            return err("ENVELOPE.RESULT", "successful result requires object data and null error")
    else:
        error = result["error"]
        required = {"code", "message", "recoverable", "suggested_action"}
        if result["data"] is not None or not isinstance(error, dict) or set(error) != required:
            return err("ENVELOPE.RESULT", "failed result requires null data and a typed error")
        if not isinstance(error["code"], str) or not isinstance(error["message"], str):
            return err("ENVELOPE.RESULT", "result error code and message must be strings")
        if not isinstance(error["recoverable"], bool):
            return err("ENVELOPE.RESULT", "result recoverable must be boolean")
        if error["suggested_action"] is not None and not isinstance(error["suggested_action"], str):
            return err("ENVELOPE.RESULT", "result suggested_action must be null or a string")
    return Ok(result)


def parse_result(text: str, correlation_id: str) -> Result[dict[str, Any], ToolError]:
    parsed = parse_single_fenced_json(text, kind="result")
    if isinstance(parsed, Err):
        return parsed
    return validate_result(parsed.value, correlation_id)


def build_attestation(
    runtime: dict[str, Any], operation: str, correlation_id: str, *,
    started_at_ms: int, finished_at_ms: int, outcome: str,
    source_sha256: str | None = None, error_code: str | None = None,
) -> Result[dict[str, Any], ToolError]:
    checked = validate_runtime(runtime)
    if isinstance(checked, Err):
        return checked
    if operation not in OPERATIONS:
        return err("ATTESTATION.OPERATION", "attestation operation is unsupported")
    if not CORRELATION_RE.fullmatch(correlation_id):
        return err("ATTESTATION.CORRELATION", "attestation correlation_id is invalid")
    if outcome not in {"success", "failure", "timeout", "canceled"}:
        return err("ATTESTATION.OUTCOME", "attestation outcome is invalid")
    if not isinstance(started_at_ms, int) or not isinstance(finished_at_ms, int) or finished_at_ms < started_at_ms:
        return err("ATTESTATION.TIMING", "attestation timing is invalid")
    if source_sha256 is not None and not SHA256_RE.fullmatch(source_sha256):
        return err("ATTESTATION.SOURCE", "attestation source digest is invalid")
    if error_code is not None and not re.fullmatch(r"[A-Z][A-Z0-9_.-]{0,63}", error_code):
        return err("ATTESTATION.ERROR", "attestation error code is invalid")
    return Ok({
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
    })


def atomic_write_json(path: str | Path, value: dict[str, Any]) -> Result[None, ToolError]:
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
        return err("ATTESTATION.WRITE", "atomic attestation write failed")
    return Ok(None)


def _emit(value: dict[str, Any]) -> None:
    print(json.dumps(value, sort_keys=True, separators=(",", ":")))


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    resolve = sub.add_parser("resolve")
    resolve.add_argument("--plugin-root", required=True)
    resolve.add_argument("--harness", required=True, choices=HARNESSES)
    resolve.add_argument("--agent", required=True, choices=tuple(AGENT_MODELS))
    check = sub.add_parser("validate-fence")
    check.add_argument("--kind", required=True, choices=("request", "result"))
    check.add_argument("--file", required=True)
    check.add_argument("--correlation-id")
    return parser


def _run(args: argparse.Namespace) -> Result[dict[str, Any], ToolError]:
    if args.command == "resolve":
        return resolve_agent(args.plugin_root, args.harness, args.agent)
    text = Path(args.file).read_text(encoding="utf-8")
    if args.kind == "request":
        return parse_request(text)
    if not args.correlation_id:
        return err("ENVELOPE.CORRELATION", "result validation requires correlation_id")
    return parse_result(text, args.correlation_id)


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        outcome = _run(args)
    except (OSError, UnicodeError):
        _emit({"success": False, "data": None, "error": {"code": "SCRIPT.IO", "message": "input could not be read", "recoverable": False, "suggested_action": None}})
        return 2
    if isinstance(outcome, Err):
        _emit({"success": False, "data": None, "error": outcome.error.to_json()})
        return 2
    _emit({"success": True, "data": outcome.value, "error": None})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

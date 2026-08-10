#!/usr/bin/env python3
"""Internal implementation of ``just smoke peer-pair``.

Run one role of the repository-owned AI.13 peer-pair smoke contract.

The runner deliberately has no host, credential, or storage knowledge.  Each
host supplies an explicit JSON configuration whose commands use the public ATM
clients.  It records evidence for a release operator and stops only daemons it
started itself.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import signal
import shutil
import socket
import subprocess
import sys
import time
from typing import Any


REQUIRED_CASES = (
    "preflight",
    "local_smoke",
    "send_read_nudge",
    "reverse_send_read_nudge",
    "requires_ack_reply",
    "duplicate_ulid",
    "unavailable_peer",
    "untrusted_or_allowlist_rejection",
    "failed_remote_ack",
)
SECRET = re.compile(r"(?i)(-----BEGIN[^-]+-----|(?:token|secret|password|capability)=?[^\s,]+)")
PUBLIC_CLIENT_COMMANDS = frozenset({"atm", "atm.exe", "atm-graft", "atm-graft.exe"})
REQUIRED_ASSERTIONS = {
    "preflight": frozenset({"daemon_ready"}),
    "local_smoke": frozenset({"receiver_visible"}),
    "send_read_nudge": frozenset({"receiver_visible", "nudge_visible"}),
    "reverse_send_read_nudge": frozenset({"receiver_visible", "nudge_visible"}),
    "requires_ack_reply": frozenset({"ack_reply_visible"}),
    "duplicate_ulid": frozenset(
        {"receiver_visible", "single_record_retained", "no_repeat_nudge", "no_ack_mutation"}
    ),
    "unavailable_peer": frozenset({"no_prohibited_delivery_state"}),
    "untrusted_or_allowlist_rejection": frozenset({"rejected_before_routing"}),
    "failed_remote_ack": frozenset({"ack_source_unchanged", "no_remote_ack_state"}),
}
MESSAGE_BOUND_ASSERTIONS = {
    "local_smoke": frozenset({"receiver_visible"}),
    "send_read_nudge": frozenset({"receiver_visible", "nudge_visible"}),
    "reverse_send_read_nudge": frozenset({"receiver_visible", "nudge_visible"}),
    "requires_ack_reply": frozenset({"ack_reply_visible"}),
    "duplicate_ulid": frozenset({"receiver_visible"}),
}


def sanitize(value: str) -> str:
    return SECRET.sub("<redacted>", value)


def fail(message: str) -> None:
    raise RuntimeError(message)


def load_config(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read peer-smoke config {path}: {error}")
    if not isinstance(value, dict):
        fail("peer-smoke config must be a JSON object")
    return value


def require_string(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        fail(f"config field `{name}` must be a non-empty string")
    return value


def require_command(value: Any, name: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        fail(f"config field `{name}` must be a non-empty argv array")
    return value


def require_public_client_command(value: Any, name: str) -> list[str]:
    command = list(require_command(value, name))
    executable = Path(command[0]).name.lower()
    if executable not in PUBLIC_CLIENT_COMMANDS:
        fail(
            f"config field `{name}` must invoke a public ATM client "
            f"({', '.join(sorted(PUBLIC_CLIENT_COMMANDS))})"
        )
    resolved = shutil.which(command[0])
    install_root = os.environ.get("ATM_SMOKE_INSTALL_ROOT")
    expected = (
        str(Path(install_root) / "bin" / executable)
        if install_root
        else shutil.which(executable)
    )
    if resolved is None or expected is None:
        fail(f"config field `{name}` cannot resolve public ATM client `{command[0]}`")
    try:
        if not Path(resolved).resolve().samefile(Path(expected).resolve()):
            fail(f"config field `{name}` does not resolve to the configured public ATM client")
    except OSError as error:
        fail(f"config field `{name}` cannot verify public ATM client identity: {error}")
    command[0] = str(Path(expected).resolve())
    return command


def require_semantic_assertion(value: Any, name: str) -> None:
    if not isinstance(value, dict):
        fail(f"config field `{name}` must be an object")
    require_public_client_command(value.get("command"), f"{name}.command")
    require_string(value.get("json_path"), f"{name}.json_path")
    if "absent" in value and value["absent"] is not True:
        fail(f"config field `{name}.absent` must be true when present")
    if value.get("absent") is True:
        if "equals" in value:
            fail(f"config field `{name}` cannot combine absent with equals")
        return
    expected = value.get("equals")
    if not isinstance(expected, (str, int, float, bool, type(None))):
        fail(f"config field `{name}.equals` must be a scalar")


LOG_EVENT_SELECTOR_KEYS = frozenset({
    "action",
    "level",
    "message",
    "outcome",
    "service",
    "target",
    "fields",
})


def require_log_event_selector(value: Any, name: str) -> None:
    if not isinstance(value, dict) or not value:
        fail(f"config field `{name}` must be a non-empty event selector object")
    if not all(isinstance(key, str) and key for key in value):
        fail(f"config field `{name}` must use non-empty string selector keys")
    unknown = sorted(set(value).difference(LOG_EVENT_SELECTOR_KEYS))
    if unknown:
        fail(f"config field `{name}` has unsupported selector keys: {', '.join(unknown)}")
    for key, expected in value.items():
        if key == "fields":
            if not isinstance(expected, dict) or not expected:
                fail(f"config field `{name}.fields` must be a non-empty object")
            if not all(
                isinstance(field, str)
                and field
                and isinstance(field_value, (str, int, float, bool, type(None)))
                and (not isinstance(field_value, str) or field_value.strip())
                for field, field_value in expected.items()
            ):
                fail(f"config field `{name}.fields` must map names to scalar values")
        elif isinstance(expected, str) and not expected.strip():
            fail(f"config field `{name}.{key}` must be a non-empty scalar")
        elif not isinstance(expected, (str, int, float, bool, type(None))):
            fail(f"config field `{name}.{key}` must be a scalar")


def require_log_event_selectors(value: Any, name: str) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        fail(f"config field `{name}` must be an array of event selector objects")
    for index, selector in enumerate(value):
        require_log_event_selector(selector, f"{name}[{index}]")
    return value


def require_semantic_verification(case: dict[str, Any], name: str, daemon_has_log_file: bool) -> None:
    verification = case.get("verification")
    if not isinstance(verification, dict):
        fail(f"config field `{name}.verification` must be an object")
    assertions = verification.get("assertions")
    if not isinstance(assertions, dict):
        fail(f"config field `{name}.verification.assertions` must be an object")
    required = REQUIRED_ASSERTIONS[case["id"]]
    missing = sorted(required.difference(assertions))
    if missing:
        fail(f"config field `{name}.verification.assertions` is missing {', '.join(missing)}")
    for assertion_name, assertion in assertions.items():
        if not isinstance(assertion_name, str) or not assertion_name:
            fail(f"config field `{name}.verification.assertions` has an invalid name")
        require_semantic_assertion(assertion, f"{name}.verification.assertions.{assertion_name}")
    for assertion_name in MESSAGE_BOUND_ASSERTIONS.get(case["id"], frozenset()):
        if assertions[assertion_name].get("equals") != "$message_ulid":
            fail(
                f"config field `{name}.verification.assertions.{assertion_name}.equals` "
                "must bind to `$message_ulid`"
            )
    if "forbidden_daemon_log_entries" in verification:
        fail(
            f"config field `{name}.verification.forbidden_daemon_log_entries` was removed; "
            "use required_daemon_log_events or forbidden_daemon_log_events"
        )
    required_log_events = require_log_event_selectors(
        verification.get("required_daemon_log_events", []),
        f"{name}.verification.required_daemon_log_events",
    )
    forbidden_log_events = require_log_event_selectors(
        verification.get("forbidden_daemon_log_events", []),
        f"{name}.verification.forbidden_daemon_log_events",
    )
    if (required_log_events or forbidden_log_events) and not daemon_has_log_file:
        fail(f"{name}.verification daemon log events require daemon.log_file")
    if case["id"] == "untrusted_or_allowlist_rejection":
        if not daemon_has_log_file:
            fail("untrusted_or_allowlist_rejection requires daemon.log_file")
        if not required_log_events:
            fail("untrusted_or_allowlist_rejection requires required_daemon_log_events")


def validate(config: dict[str, Any]) -> None:
    if config.get("schema_version") != 1:
        fail("config field `schema_version` must be 1")
    if config.get("role") not in {"A", "B"}:
        fail("config field `role` must be A or B")
    require_string(config.get("commit"), "commit")
    daemon = config.get("daemon")
    if not isinstance(daemon, dict):
        fail("config field `daemon` must be an object")
    require_string(daemon.get("endpoint"), "daemon.endpoint")
    require_public_client_command(daemon.get("version_command"), "daemon.version_command")
    require_public_client_command(config.get("client_version_command"), "client_version_command")
    security = config.get("peer_security")
    if not isinstance(security, dict):
        fail("config field `peer_security` must be an object")
    require_string(security.get("trust_id"), "peer_security.trust_id")
    require_string(security.get("certificate_fingerprint"), "peer_security.certificate_fingerprint")
    if daemon.get("launch_command") is not None:
        require_command(daemon["launch_command"], "daemon.launch_command")
    if daemon.get("log_file") is not None:
        require_string(daemon["log_file"], "daemon.log_file")
    identities = config.get("identities")
    if not isinstance(identities, dict):
        fail("config field `identities` must be an object")
    for field in ("sender", "recipient"):
        require_string(identities.get(field), f"identities.{field}")
    cases = config.get("cases")
    if not isinstance(cases, list) or [case.get("id") for case in cases if isinstance(case, dict)] != list(REQUIRED_CASES):
        fail("config cases must contain the required ordered AI.13 case IDs exactly once")
    for case in cases:
        if not isinstance(case, dict):
            fail("each case must be an object")
        require_public_client_command(case.get("command"), f"cases.{case.get('id', '<unknown>')}.command")
        if case.get("expect") not in {"success", "typed_error"}:
            fail("each case expect must be `success` or `typed_error`")
        if case["expect"] == "typed_error":
            require_string(case.get("typed_error_code"), f"cases.{case.get('id', '<unknown>')}.typed_error_code")
        require_string(case.get("message_ulid"), f"cases.{case.get('id', '<unknown>')}.message_ulid")
        require_semantic_verification(
            case,
            f"cases.{case.get('id', '<unknown>')}",
            isinstance(daemon.get("log_file"), str),
        )
    paths = daemon.get("owned_runtime_paths", [])
    if not isinstance(paths, list) or not all(isinstance(path, str) for path in paths):
        fail("daemon.owned_runtime_paths must be an array of paths")
    if daemon.get("launch_command") is not None:
        if not paths:
            fail("daemon.launch_command requires daemon.owned_runtime_paths")
        require_string(daemon.get("runtime_dir"), "daemon.runtime_dir")
    elif paths:
        fail("daemon.owned_runtime_paths requires daemon.launch_command")


def run_command(command: list[str], timeout: float) -> dict[str, Any]:
    completed = subprocess.run(
        command, capture_output=True, text=True, encoding="utf-8", errors="replace",
        check=False, timeout=timeout,
    )
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": sanitize(completed.stdout)[-8192:],
        "stderr": sanitize(completed.stderr)[-8192:],
    }


def log_window(raw_path: Any) -> str:
    if not isinstance(raw_path, str) or not raw_path:
        return ""
    try:
        return sanitize(Path(raw_path).read_text(encoding="utf-8", errors="replace"))[-8192:]
    except OSError as error:
        return f"<unavailable: {sanitize(str(error))}>"


def log_snapshot(raw_path: Any) -> str:
    if not isinstance(raw_path, str) or not raw_path:
        return ""
    try:
        return Path(raw_path).read_text(encoding="utf-8", errors="replace")
    except OSError as error:
        fail(f"cannot read daemon log for semantic verification: {error}")


def json_path(value: Any, path: str) -> Any:
    current = value
    for segment in path.split("."):
        if isinstance(current, dict) and segment in current:
            current = current[segment]
        elif isinstance(current, list) and segment.isdecimal() and int(segment) < len(current):
            current = current[int(segment)]
        else:
            fail(f"semantic verification JSON path `{path}` was absent")
    return current


def resolved_expectation(value: Any, case: dict[str, Any]) -> Any:
    return case["message_ulid"] if value == "$message_ulid" else value


def daemon_log_event_matches(event: dict[str, Any], selector: dict[str, Any]) -> bool:
    fields = event.get("fields")
    nested_fields = fields if isinstance(fields, dict) else {}
    for key, expected in selector.items():
        if key == "fields":
            if not all(nested_fields.get(field) == value for field, value in expected.items()):
                return False
            continue
        observed = event.get(key)
        if observed is None and key in nested_fields:
            observed = nested_fields[key]
        if observed != expected:
            return False
    return True


def parse_daemon_log_events(delta: str) -> tuple[list[dict[str, Any]], int]:
    events: list[dict[str, Any]] = []
    malformed_lines = 0
    for line in delta.splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            malformed_lines += 1
            continue
        if isinstance(event, dict):
            events.append(event)
        else:
            malformed_lines += 1
    return events, malformed_lines


def verify_daemon_log_events(
    verification: dict[str, Any],
    daemon_log_before: str,
    daemon_log_file: Any,
    outcome: dict[str, Any],
) -> None:
    required = verification.get("required_daemon_log_events", [])
    forbidden = verification.get("forbidden_daemon_log_events", [])
    if not required and not forbidden:
        return
    after = log_snapshot(daemon_log_file)
    if not after.startswith(daemon_log_before):
        outcome["failures"].append(
            "daemon log rotated or truncated during semantic verification; refusing an unverified delta"
        )
        outcome["daemon_log_delta"] = sanitize(after)[-8192:]
        return
    delta = after[len(daemon_log_before) :]
    events, malformed_lines = parse_daemon_log_events(delta)
    outcome["daemon_log_delta"] = sanitize(delta)[-8192:]
    outcome["daemon_log_event_count"] = len(events)
    outcome["daemon_log_malformed_lines"] = malformed_lines
    if malformed_lines:
        outcome["failures"].append(
            f"daemon log delta contains {malformed_lines} non-JSON line(s); "
            "cannot verify structured daemon log events"
        )
    for selector in required:
        if not any(daemon_log_event_matches(event, selector) for event in events):
            outcome["failures"].append(
                f"daemon log did not contain required structured rejection event {selector!r}"
            )
    for selector in forbidden:
        if any(daemon_log_event_matches(event, selector) for event in events):
            outcome["failures"].append(
                f"daemon log recorded forbidden structured event {selector!r}"
            )


def verify_semantics(
    case: dict[str, Any], timeout: float, daemon_log_before: str, daemon_log_file: Any
) -> dict[str, Any]:
    verification = case["verification"]
    outcome: dict[str, Any] = {"assertions": {}, "status": "fail", "failures": []}
    for assertion_name, assertion in verification["assertions"].items():
        result = run_command(assertion["command"], timeout)
        assertion_outcome: dict[str, Any] = {"result": result}
        outcome["assertions"][assertion_name] = assertion_outcome
        if result["exit_code"] != 0:
            outcome["failures"].append(f"semantic assertion `{assertion_name}` command failed")
            continue
        try:
            observed = json.loads(result["stdout"])
            actual = json_path(observed, assertion["json_path"])
        except (json.JSONDecodeError, RuntimeError) as error:
            if assertion.get("absent") is True and isinstance(error, RuntimeError):
                assertion_outcome["observed"] = "absent"
                continue
            outcome["failures"].append(f"semantic assertion `{assertion_name}` failed: {error}")
            continue
        if assertion.get("absent") is True:
            outcome["failures"].append(
                f"semantic assertion `{assertion_name}` expected `{assertion['json_path']}` to be absent"
            )
            continue
        expected = resolved_expectation(assertion["equals"], case)
        assertion_outcome.update({"observed": actual, "expected": expected})
        if actual != expected:
            outcome["failures"].append(
                f"semantic assertion `{assertion_name}` expected {expected!r}, got {actual!r}"
            )
    verify_daemon_log_events(verification, daemon_log_before, daemon_log_file, outcome)
    if not outcome["failures"]:
        outcome["status"] = "pass"
    return outcome


def endpoint_closed(endpoint: str) -> bool:
    host, separator, port = endpoint.rpartition(":")
    if not separator or not port.isdecimal():
        return True  # UDS/endpoints without TCP syntax are verified by PID ownership only.
    try:
        with socket.create_connection((host.strip("[]"), int(port)), timeout=0.25):
            return False
    except OSError:
        return True


def stop_owned(process: subprocess.Popen[str] | None, config: dict[str, Any]) -> dict[str, Any]:
    result: dict[str, Any] = {"launched_pid": process.pid if process else None, "status": "not_owned"}
    daemon = config["daemon"]
    if process is None and daemon.get("launch_command") is None:
        return result
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    listener_closed = endpoint_closed(daemon["endpoint"])
    result.update({"status": "stopped" if listener_closed else "listener_remaining", "listener_closed": listener_closed})
    if not listener_closed:
        return result
    runtime_dir = Path(daemon["runtime_dir"]).resolve()
    marker = runtime_dir / ".peer-smoke-owned"
    expected_marker = str(process.pid) if process is not None else "pending"
    if not marker.is_file() or marker.read_text(encoding="utf-8").strip() != expected_marker:
        result.update({"status": "ownership_marker_missing", "listener_closed": listener_closed})
        return result
    try:
        for raw_path in daemon.get("owned_runtime_paths", []):
            path = Path(raw_path).resolve()
            if runtime_dir not in path.parents:
                result.update({"status": "unsafe_runtime_path", "listener_closed": listener_closed})
                return result
            if path.is_file() or path.is_socket():
                path.unlink()
        marker.unlink()
        runtime_dir.rmdir()
    except OSError as error:
        result.update({"status": "cleanup_failed", "cleanup_error": sanitize(str(error))})
    return result


def git_commit() -> str:
    completed = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
    return completed.stdout.strip() if completed.returncode == 0 else "unknown"


def execute(config: dict[str, Any], evidence_dir: Path, timeout: float) -> int:
    evidence_dir.mkdir(parents=True, exist_ok=True)
    daemon = config["daemon"]
    process: subprocess.Popen[str] | None = None
    records: list[dict[str, Any]] = []
    status = "passed"
    try:
        if daemon.get("launch_command"):
            runtime_dir = Path(daemon["runtime_dir"])
            if runtime_dir.exists():
                fail(f"runner-owned runtime_dir already exists: {runtime_dir}")
            runtime_dir.mkdir(parents=True)
            (runtime_dir / ".peer-smoke-owned").write_text("pending", encoding="utf-8")
        if daemon.get("launch_command"):
            process = subprocess.Popen(
                daemon["launch_command"], text=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            (Path(daemon["runtime_dir"]) / ".peer-smoke-owned").write_text(
                str(process.pid), encoding="utf-8"
            )
        version = run_command(daemon["version_command"], timeout)
        client_version = run_command(config["client_version_command"], timeout)
        for case in config["cases"]:
            daemon_log_before = log_snapshot(daemon.get("log_file"))
            result = run_command(case["command"], timeout)
            if case["expect"] == "success":
                transport_passed = result["exit_code"] == 0
            else:
                transport_passed = result["exit_code"] != 0 and case["typed_error_code"] in (result["stdout"] + result["stderr"])
            semantic = verify_semantics(case, timeout, daemon_log_before, daemon.get("log_file"))
            passed = transport_passed and semantic["status"] == "pass"
            record = {
                "schema_version": 1,
                "commit": config["commit"],
                "runner_commit": git_commit(),
                "role": config["role"],
                "daemon_endpoint": daemon["endpoint"],
                "daemon_version": version,
                "client_version": client_version,
                "peer_security": config["peer_security"],
                "sender": config["identities"]["sender"],
                "recipient": config["identities"]["recipient"],
                "transport": "https-mtls",
                "case": case["id"],
                "message_ulid": case["message_ulid"],
                "expected": case["expect"],
                "expected_error_code": case.get("typed_error_code"),
                "result": result,
                "semantic_verification": semantic,
                "daemon_log_window": log_window(daemon.get("log_file")),
                "status": "pass" if passed else "fail",
            }
            records.append(record)
            if not passed:
                status = "failed"
                break
    except (OSError, subprocess.TimeoutExpired, RuntimeError) as error:
        status = "failed"
        records.append({"schema_version": 1, "status": "fail", "error": sanitize(str(error))})
    finally:
        teardown = stop_owned(process, config)
        if teardown["status"] == "listener_remaining":
            status = "failed"
        for record in records:
            record["teardown"] = teardown
        (evidence_dir / "peer-smoke-evidence.json").write_text(
            json.dumps({"status": status, "records": records}, indent=2) + "\n",
            encoding="utf-8",
        )
    return 0 if status == "passed" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--evidence-dir", required=True, type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=15.0)
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    config = load_config(args.config)
    validate(config)
    if args.validate_only:
        return 0
    return execute(config, args.evidence_dir, args.timeout_seconds)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as error:
        print(f"peer-smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)

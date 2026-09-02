#!/usr/bin/env python3
"""Capture the AV.1b live read-under-writer-stall proof.

The operator owns daemon lifecycle and the isolated-account writer lock.  This
tool uses only the public ``atm`` CLI: it never starts, stops, switches, or
otherwise configures a daemon.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_BUDGET_MS = 3_000
LOG_LIMIT = 80


class ProofError(RuntimeError):
    """A failed precondition or proof assertion."""


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def run_command(command: list[str], timeout_seconds: float) -> dict[str, Any]:
    """Run one public CLI command and retain bounded, JSON-safe evidence."""
    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": command, "exit_code": None, "stdout": "", "stderr": str(error)}
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def parse_json(result: dict[str, Any], label: str) -> dict[str, Any]:
    if result["exit_code"] != 0:
        raise ProofError(f"{label} failed: {result['stderr'].strip() or result['stdout'].strip()}")
    try:
        parsed = json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise ProofError(f"{label} did not emit JSON: {error}") from error
    if not isinstance(parsed, dict):
        raise ProofError(f"{label} emitted a JSON value instead of an object")
    return parsed


def branch_version() -> str:
    """Return the one release version shared by this checkout's CLI + daemon."""
    result = run_command(["cargo", "metadata", "--no-deps", "--format-version", "1"], 20.0)
    metadata = parse_json(result, "cargo metadata")
    versions = {
        package.get("version")
        for package in metadata.get("packages", [])
        if isinstance(package, dict) and package.get("name") in {"agent-team-mail", "atm-daemon"}
    }
    if len(versions) != 1 or not all(isinstance(version, str) and version for version in versions):
        raise ProofError("checkout does not expose one shared atm + atm-daemon version")
    return versions.pop()


def doctor_is_matched_ready(report: dict[str, Any], expected_version: str) -> tuple[bool, str]:
    """Require the public paired CLI/daemon readiness contract before a proof."""
    summary = report.get("summary")
    runtime = report.get("runtime_status")
    client = report.get("client_context")
    daemon = report.get("daemon_context")
    if not isinstance(summary, dict) or summary.get("status") != "healthy":
        return False, "doctor summary is not healthy"
    if not isinstance(runtime, dict) or runtime.get("readiness") != "ready":
        return False, "daemon runtime is not ready"
    if not isinstance(client, dict) or client.get("version") != expected_version:
        return False, "CLI version does not match this checkout"
    if not isinstance(daemon, dict) or daemon.get("version") != expected_version:
        return False, "daemon version does not match this checkout"
    return True, "matched CLI/daemon pair is ready"


def read_command(args: argparse.Namespace) -> list[str]:
    command = [args.atm, "read", "--team", args.team, "--as", args.actor, "--unread-only", "--json"]
    if args.message_id:
        command.extend(["--message-id", args.message_id])
    # ATM's public CLI timeout parser accepts whole seconds.  Keep the
    # evidence budget in milliseconds for measured-latency precision, while
    # refusing a value we could not actually enforce at the CLI boundary.
    command.extend(["--timeout", str(args.budget_ms // 1_000)])
    return command


def doctor_command(atm: str, team: str) -> list[str]:
    return [atm, "doctor", "--team", team, "--json"]


def log_command(atm: str, started_at: str) -> list[str]:
    return [atm, "log", "snapshot", "--since", started_at, "--limit", str(LOG_LIMIT), "--json"]


def measure_read(args: argparse.Namespace) -> dict[str, Any]:
    started = time.monotonic()
    result = run_command(read_command(args), args.budget_ms / 1_000 + 1.0)
    elapsed_ms = (time.monotonic() - started) * 1_000
    within_budget = result["exit_code"] == 0 and elapsed_ms <= args.budget_ms
    return {
        "command": result.get("command", read_command(args)),
        "exit_code": result["exit_code"],
        "elapsed_ms": round(elapsed_ms, 3),
        "budget_ms": args.budget_ms,
        "within_budget": within_budget,
        "stdout": result["stdout"],
        "stderr": result["stderr"],
    }


def capture_logs(atm: str, started_at: str) -> dict[str, Any]:
    result = run_command(log_command(atm, started_at), 10.0)
    if result["exit_code"] != 0:
        return {"captured": False, "error": result["stderr"].strip() or result["stdout"].strip()}
    try:
        return {"captured": True, "entries": json.loads(result["stdout"])}
    except json.JSONDecodeError as error:
        return {"captured": False, "error": f"log snapshot did not emit JSON: {error}"}


def write_evidence(path: Path, record: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(f"{path.suffix}.tmp")
    temporary.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="AV.1b read-under-writer-stall proof (daemon lifecycle is operator-owned).")
    parser.add_argument("--team", required=True, help="team containing the isolated proof mailbox")
    parser.add_argument("--actor", required=True, help="isolated proof mailbox agent")
    parser.add_argument("--message-id", help="optional known unread message ID to read")
    parser.add_argument("--atm", default=os.environ.get("ATM_AV1B_ATM", "atm"), help="matched CLI binary")
    parser.add_argument("--budget-ms", type=int, default=DEFAULT_BUDGET_MS, help="external read deadline budget (default: 3000)")
    parser.add_argument("--evidence-out", type=Path, required=True, help="JSON evidence file to create or replace")
    args = parser.parse_args(argv)
    if args.budget_ms <= 0 or args.budget_ms % 1_000:
        parser.error("--budget-ms must be a positive whole number of seconds")
    return args


def execute(args: argparse.Namespace) -> dict[str, Any]:
    expected_version = branch_version()
    started_at = _utc_now()
    record: dict[str, Any] = {
        "schema": "av1b-read-under-stall-v1",
        "started_at": started_at,
        "expected_version": expected_version,
        "budget_ms": args.budget_ms,
        "target": {"team": args.team, "actor": args.actor, "message_id": args.message_id},
        "daemon_lifecycle": "operator_owned; this tool invokes only atm doctor/read/log",
    }
    try:
        before = parse_json(run_command(doctor_command(args.atm, args.team), 10.0), "pre-proof doctor")
        record["doctor_before"] = before
        ready, reason = doctor_is_matched_ready(before, expected_version)
        if not ready:
            raise ProofError(reason)
        record["read"] = measure_read(args)
        record["doctor_after"] = parse_json(
            run_command(doctor_command(args.atm, args.team), 10.0), "post-proof doctor"
        )
        record["retained_log_excerpt"] = capture_logs(args.atm, started_at)
        record["status"] = "PASS" if record["read"]["within_budget"] else "FAIL"
        if record["status"] == "FAIL":
            record["failure"] = "atm read did not succeed within the configured deadline budget"
    except ProofError as error:
        record["status"] = "FAIL"
        record["failure"] = str(error)
    record["finished_at"] = _utc_now()
    return record


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    record = execute(args)
    write_evidence(args.evidence_out, record)
    print(f"{record['status']} AV.1b read-under-stall proof: {args.evidence_out}")
    return 0 if record["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())

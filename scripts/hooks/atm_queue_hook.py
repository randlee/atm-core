#!/usr/bin/env python3
"""Claude/Codex lifecycle hooks for ATM heartbeat and queue pulls."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def state_path(*, create: bool = True) -> Path:
    root = Path(os.environ.get("ATM_HOOK_STATE_DIR", ".atm-hook-state"))
    if create:
        root.mkdir(parents=True, exist_ok=True)
    return root / "pending-idle"


def atm_command() -> str:
    return os.environ.get("ATM_BIN", "atm")


def atm_command_parts() -> list[str]:
    """Build a portable argv for the configured ATM executable.

    Unix can execute a Python test shim through its shebang, but Windows does
    not treat a `.py` path as an executable for `CreateProcess`. Supporting a
    Python shim here keeps the deterministic hook harness cross-platform while
    leaving normal `atm`/`atm.exe` launches unchanged.
    """
    command = atm_command()
    if Path(command).suffix.lower() == ".py":
        return [sys.executable, command]
    return [command]


def write_hook_trace(
    command: list[str],
    completed: subprocess.CompletedProcess[str] | None = None,
    error: str | None = None,
) -> None:
    """Persist optional raw command diagnostics for live evidence harnesses."""
    trace_file = os.environ.get("ATM_HOOK_TRACE_FILE", "").strip()
    if not trace_file:
        return
    payload: dict[str, object] = {"argv": command}
    if completed is not None:
        payload.update(
            {
                "returncode": completed.returncode,
                "stdout": completed.stdout,
                "stderr": completed.stderr,
            }
        )
    if error is not None:
        payload["error"] = error
    try:
        Path(trace_file).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    except OSError:
        # Diagnostics must never change the lifecycle hook's delivery result.
        pass


def run_atm(
    args: list[str], *, strict: bool = False, trace: bool = False
) -> subprocess.CompletedProcess[str] | None:
    command = [*atm_command_parts(), *args]
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=float(os.environ.get("ATM_HOOK_TIMEOUT_SECONDS", "2")),
            env=os.environ.copy(),
        )
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        if trace:
            write_hook_trace(command, error=str(error))
        if strict:
            raise RuntimeError(f"could not run {' '.join(command)}: {error}") from error
        return None
    if trace:
        write_hook_trace(command, completed)
    if strict and completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip() or "no diagnostic output"
        raise RuntimeError(
            f"{' '.join(command)} exited {completed.returncode}: {detail}"
        )
    return completed


def stop_hook_context() -> tuple[str, str]:
    identity = os.environ.get("ATM_IDENTITY", "").strip()
    team = os.environ.get("ATM_TEAM", "").strip()
    if not identity:
        raise RuntimeError("ATM_IDENTITY is required for a Stop queue pull")
    if not team:
        raise RuntimeError("ATM_TEAM is required for a Stop queue pull")
    home = os.environ.get("ATM_HOME") or os.environ.get("HOME")
    if not home:
        raise RuntimeError("ATM_HOME or HOME is required for a Stop queue pull")
    home_path = Path(home)
    if not home_path.is_absolute() or not home_path.is_dir():
        raise RuntimeError(f"ATM home is not an existing absolute directory: {home}")
    if not atm_command().strip():
        raise RuntimeError("ATM_BIN must name the ATM CLI for a Stop queue pull")
    return team, identity


def queue_pull(harness: str) -> list[dict[str, object]]:
    team, identity = stop_hook_context()
    response = run_atm(
        [
            "_internal-queue-get",
            "--team",
            team,
            "--as",
            identity,
            "--require-daemon",
        ],
        strict=True,
        trace=True,
    )
    if response is None:
        raise RuntimeError("ATM queue pull returned no process result")
    messages: list[dict[str, object]] = []
    for line in response.stdout.splitlines():
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"ATM queue pull returned invalid JSON: {error}") from error
        if not isinstance(value, dict):
            raise RuntimeError("ATM queue pull returned a non-object JSON value")
        messages.append(value)
    return messages


def send_heartbeat(activity: str, harness: str) -> None:
    run_atm(["_internal-heartbeat", "--activity", activity, "--as", os.environ.get("ATM_IDENTITY", "")])


def cancel_idle() -> None:
    try:
        state_path(create=False).unlink()
    except FileNotFoundError:
        pass


def schedule_idle(harness: str) -> None:
    token = f"{time.time_ns()}"
    path = state_path()
    path.write_text(token, encoding="utf-8")
    delay = max(0.0, float(os.environ.get("ATM_HOOK_DEBOUNCE_SECONDS", "2")))
    if delay == 0:
        expire_idle(token, harness)
        return
    child = subprocess.Popen(  # noqa: S603 - ATM_BIN is an explicit operator override
        [sys.executable, __file__, "--event", "idle-expired", "--harness", harness, "--token", token],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        env=os.environ.copy(),
    )
    # Test-support hook only: this detached child is deliberately never
    # waited on by the hook process itself (it must survive past this
    # process's own exit so the idle debounce fires later). When a
    # deterministic test needs to prove the child has fully exited before
    # tearing down a temp directory it may still be touching, it can set
    # ATM_HOOK_DEBOUNCE_CHILD_PIDFILE and poll the recorded pid itself.
    # Inert unless that env var is set; changes no production behavior.
    pidfile = os.environ.get("ATM_HOOK_DEBOUNCE_CHILD_PIDFILE", "").strip()
    if pidfile:
        Path(pidfile).write_text(str(child.pid), encoding="utf-8")


def expire_idle(token: str, harness: str) -> None:
    delay = max(0.0, float(os.environ.get("ATM_HOOK_DEBOUNCE_SECONDS", "2")))
    if delay:
        time.sleep(delay)
    # The hook process can outlive the host application's temporary state
    # directory. Never recreate that directory from a detached expiry child.
    path = state_path(create=False)
    try:
        if path.read_text(encoding="utf-8") != token:
            return
        path.unlink()
    except FileNotFoundError:
        return
    send_heartbeat("idle", harness)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", choices=("pre-tool-use", "stop", "session-end", "idle-expired"), required=True)
    parser.add_argument("--harness", choices=("claude", "codex"), default="claude")
    parser.add_argument("--token")
    args = parser.parse_args()
    if args.event == "idle-expired":
        expire_idle(args.token or "", args.harness)
        return 0
    if args.event == "pre-tool-use":
        cancel_idle()
        send_heartbeat("active-tool-use", args.harness)
        return 0
    if args.event == "session-end":
        cancel_idle()
        send_heartbeat("session-ended", args.harness)
        return 0
    try:
        messages = queue_pull(args.harness)
    except RuntimeError as error:
        print(f"ATM Stop queue hook failed: {error}", file=sys.stderr)
        return 1
    schedule_idle(args.harness)
    if messages and args.harness == "claude":
        reason = "\n".join(str(message.get("body", "")) for message in messages)
        print(json.dumps({"decision": "block", "reason": reason}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

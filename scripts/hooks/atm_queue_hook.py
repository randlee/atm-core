#!/usr/bin/env python3
"""Fail-open Claude/Codex lifecycle hooks for ATM heartbeat and queue pulls."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def state_path() -> Path:
    root = Path(os.environ.get("ATM_HOOK_STATE_DIR", ".atm-hook-state"))
    root.mkdir(parents=True, exist_ok=True)
    return root / "pending-idle"


def atm_command() -> str:
    return os.environ.get("ATM_BIN", "atm")


def run_atm(args: list[str]) -> subprocess.CompletedProcess[str] | None:
    try:
        return subprocess.run(
            [atm_command(), *args],
            check=False,
            capture_output=True,
            text=True,
            timeout=float(os.environ.get("ATM_HOOK_TIMEOUT_SECONDS", "2")),
            env=os.environ.copy(),
        )
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def send_heartbeat(activity: str, harness: str) -> None:
    run_atm(["_internal-heartbeat", "--activity", activity, "--as", os.environ.get("ATM_IDENTITY", "")])


def cancel_idle() -> None:
    try:
        state_path().unlink()
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
    subprocess.Popen(  # noqa: S603 - ATM_BIN is an explicit operator override
        [sys.executable, __file__, "--event", "idle-expired", "--harness", harness, "--token", token],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        env=os.environ.copy(),
    )


def expire_idle(token: str, harness: str) -> None:
    delay = max(0.0, float(os.environ.get("ATM_HOOK_DEBOUNCE_SECONDS", "2")))
    if delay:
        time.sleep(delay)
    path = state_path()
    try:
        if path.read_text(encoding="utf-8") != token:
            return
        path.unlink()
    except FileNotFoundError:
        return
    send_heartbeat("idle", harness)


def queue_pull(harness: str) -> list[dict[str, object]]:
    response = run_atm(["_internal-queue-get"])
    if response is None or response.returncode != 0:
        return []
    messages: list[dict[str, object]] = []
    for line in response.stdout.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            messages.append(value)
    return messages


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
    messages = queue_pull(args.harness)
    schedule_idle(args.harness)
    if messages and args.harness == "claude":
        reason = "\n".join(str(message.get("body", "")) for message in messages)
        print(json.dumps({"decision": "block", "reason": reason}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import os
import signal
import subprocess
import time


def process_is_alive(pid: int) -> bool:
    if os.name == "nt":
        completed = subprocess.run(
            ["tasklist", "/FI", f"PID eq {pid}"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        return completed.returncode == 0 and str(pid) in completed.stdout
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def count_atm_daemon_processes() -> list[int]:
    if os.name == "nt":
        completed = subprocess.run(
            ["tasklist", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        pids: list[int] = []
        for line in completed.stdout.splitlines():
            if not line.lower().startswith('"atm-daemon.exe"'):
                continue
            columns = [item.strip('"') for item in line.split('","')]
            if len(columns) > 1 and columns[1].isdigit():
                pids.append(int(columns[1]))
        return pids
    completed = subprocess.run(
        # The physical benchmark account must be isolated from the interactive
        # account, not from every ATM daemon on the machine.  ``ps -x`` limits
        # this scan to the executing OS user; global ``ps -ax`` incorrectly
        # rejected a safe account whenever another user ran ATM.
        ["ps", "-x", "-o", "pid=,command="],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        return []
    pids: list[int] = []
    daemon_name = "atm-daemon.exe" if os.name == "nt" else "atm-daemon"
    for line in completed.stdout.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        parts = stripped.split(None, 1)
        if len(parts) != 2 or not parts[0].isdigit():
            continue
        pid = int(parts[0])
        executable = parts[1].split(None, 1)[0]
        if os.path.basename(executable) == daemon_name:
            pids.append(pid)
    return pids


def terminate_process(pid: int | None) -> None:
    if pid is None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(pid), "/T", "/F"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return


def wait_for_process_exit(
    pid: int | None,
    *,
    timeout_seconds: float = 10.0,
    process_label: str = "atm-daemon",
) -> None:
    if pid is None:
        return
    deadline = time.monotonic() + timeout_seconds
    while process_is_alive(pid) and time.monotonic() < deadline:
        time.sleep(0.1)
    if process_is_alive(pid):
        raise RuntimeError(
            f"{process_label} pid {pid} did not exit within {timeout_seconds}s"
        )


def require_clean_host_daemon_state(*, smoke_label: str) -> None:
    if os.name == "nt":
        completed = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq atm-daemon.exe"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        active = "atm-daemon.exe" in completed.stdout.lower()
    else:
        active = bool(count_atm_daemon_processes())
    if active:
        raise RuntimeError(
            f"{smoke_label} requires an isolated OS user with no existing "
            "atm-daemon; refusing to attach to or terminate an ambient daemon"
        )


def assert_no_process_leak(
    before: list[int],
    after: list[int],
    *,
    smoke_label: str,
) -> None:
    leaked = sorted(set(after) - set(before))
    if leaked:
        raise RuntimeError(
            f"{smoke_label} detected leaked atm-daemon pid(s): {', '.join(str(pid) for pid in leaked)}"
        )

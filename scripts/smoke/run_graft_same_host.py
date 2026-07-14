#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from fixtures import create_clean_room_fixture, repo_root, smoke_env


def debug_binary(root: Path, name: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    return root / "target" / "debug" / f"{name}{suffix}"


def smoke_binary(root: Path, name: str) -> Path:
    install_root = os.environ.get("ATM_SMOKE_INSTALL_ROOT")
    if not install_root:
        return debug_binary(root, name)
    binary = Path(install_root) / "bin" / f"{name}{'.exe' if os.name == 'nt' else ''}"
    if not binary.is_file():
        raise RuntimeError(f"installed-artifact smoke requires {binary}")
    return binary


def ensure_debug_binaries(root: Path) -> None:
    if os.environ.get("ATM_SMOKE_INSTALL_ROOT"):
        return
    completed = subprocess.run(
        ["cargo", "build", "-p", "agent-team-mail", "-p", "atm-daemon", "-p", "atm-graft", "--example", "smoke_same_host"],
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            json.dumps(
                {
                    "command": completed.args,
                    "exit_code": completed.returncode,
                    "stdout": completed.stdout.strip(),
                    "stderr": completed.stderr.strip(),
                },
                indent=2,
            )
        )


def run_atm_result(
    root: Path, env: dict[str, str], cwd: Path, *args: str, stdin: str | None = None
) -> dict[str, object]:
    command = [str(smoke_binary(root, "atm")), *args]
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
        timeout=30,
        input=stdin,
    )
    return {
        "command": command,
        "cwd": str(cwd),
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }


def run_atm(
    root: Path, env: dict[str, str], cwd: Path, *args: str, stdin: str | None = None
) -> str:
    completed = run_atm_result(root, env, cwd, *args, stdin=stdin)
    if completed["exit_code"] != 0:
        raise RuntimeError(json.dumps(completed, indent=2))
    return str(completed["stdout"])


def parse_json_output(raw: str) -> dict[str, object]:
    return json.loads(raw)


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
        ["pgrep", "-f", r"(^|/)atm-daemon( |$)"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        return []
    return [int(line) for line in completed.stdout.splitlines() if line.strip().isdigit()]


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


def wait_for_process_exit(pid: int | None, timeout_seconds: float = 10.0) -> None:
    if pid is None:
        return
    deadline = time.monotonic() + timeout_seconds
    while process_is_alive(pid) and time.monotonic() < deadline:
        time.sleep(0.1)
    if process_is_alive(pid):
        raise RuntimeError(
            f"graft smoke daemon pid {pid} did not exit within {timeout_seconds}s"
        )


def require_clean_host_daemon_state() -> None:
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
        completed = subprocess.run(
            ["pgrep", "-f", r"(^|/)atm-daemon( |$)"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        active = completed.returncode == 0
    if active:
        raise RuntimeError(
            "graft same-host smoke requires an isolated OS user with no existing "
            "atm-daemon; refusing to attach to or terminate an ambient daemon"
        )


def assert_no_process_leak(before: list[int], after: list[int]) -> None:
    leaked = sorted(set(after) - set(before))
    if leaked:
        raise RuntimeError(
            f"graft same-host smoke detected leaked atm-daemon pid(s): {', '.join(str(pid) for pid in leaked)}"
        )


def wait_for_file(path: Path, timeout_seconds: float = 30.0) -> None:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for graft ready file: {path}")


def ensure_member(root: Path, env: dict[str, str], workspace_dir: Path, team: str, member: str) -> None:
    completed = run_atm_result(
        root,
        env,
        workspace_dir,
        "teams",
        "add-member",
        team,
        member,
        "--json",
    )
    if completed["exit_code"] == 0:
        return
    stderr = str(completed.get("stderr", ""))
    if "already exists in team" in stderr:
        return
    raise RuntimeError(json.dumps(completed, indent=2))


def main() -> int:
    root = repo_root()
    daemon_pids_before = count_atm_daemon_processes()
    require_clean_host_daemon_state()
    ensure_debug_binaries(root)

    unique = next(tempfile._get_candidate_names()).replace("_", "")[:8]
    team_name = f"z21-graft-{unique}"
    operator = f"z21-graft-operator-{unique}"
    graft_agent = f"z21-graft-host-{unique}"
    fixture = create_clean_room_fixture(
        prefix="z21g.",
        team_name=team_name,
        operator=operator,
        recipient=graft_agent,
    )
    daemon_pid: int | None = None
    example_proc: subprocess.Popen[str] | None = None
    try:
        atm_toml = fixture.workspace_dir / ".atm.toml"
        atm_toml.write_text(
            atm_toml.read_text(encoding="utf-8") + "\n[atm.graft]\nenabled = true\n",
            encoding="utf-8",
        )
        operator_env = smoke_env(fixture, identity=operator, root=root)
        operator_env["ATM_DAEMON_BIN"] = str(smoke_binary(root, "atm-daemon"))
        graft_env = smoke_env(fixture, identity=graft_agent, root=root)
        graft_env["ATM_DAEMON_BIN"] = str(smoke_binary(root, "atm-daemon"))

        ensure_member(root, operator_env, fixture.workspace_dir, team_name, operator)
        ensure_member(root, operator_env, fixture.workspace_dir, team_name, graft_agent)

        doctor = parse_json_output(run_atm(root, operator_env, fixture.workspace_dir, "doctor", "--json"))
        owner_pid = doctor.get("runtime_status", {}).get("singleton_owner_pid")
        daemon_pid = int(owner_pid) if owner_pid is not None else None
        daemon_pids_during = count_atm_daemon_processes()

        ready_file = fixture.root / "graft-ready.txt"
        example_command = [
            "cargo",
            "run",
            "-p",
            "atm-graft",
            "--example",
            "smoke_same_host",
            "--",
            str(fixture.workspace_dir),
            team_name,
            graft_agent,
            f"{operator}@{team_name}",
            "graft smoke request",
            operator,
            str(ready_file),
        ]
        example_proc = subprocess.Popen(
            example_command,
            cwd=root,
            env=graft_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        wait_for_file(ready_file)

        initial_send = parse_json_output(
            run_atm(
                root,
                operator_env,
                fixture.workspace_dir,
                "send",
                f"{graft_agent}@{team_name}",
                "graft smoke request",
                "--requires-ack",
                "--json",
            )
        )

        stdout, stderr = example_proc.communicate(timeout=60)
        if example_proc.returncode != 0:
            raise RuntimeError(
                json.dumps(
                    {
                        "command": example_command,
                        "exit_code": example_proc.returncode,
                        "stdout": stdout.strip(),
                        "stderr": stderr.strip(),
                    },
                    indent=2,
                )
            )
        example_payload = json.loads(stdout)
        if example_payload.get("status") != "passed":
            raise RuntimeError(json.dumps(example_payload, indent=2))

        follow_up_message_id = str(example_payload["follow_up_message_id"])
        follow_up_read = parse_json_output(
            run_atm(
                root,
                operator_env,
                fixture.workspace_dir,
                "read",
                "--team",
                team_name,
                "--all",
                "--message-id",
                follow_up_message_id,
                "--json",
            )
        )
        if follow_up_read.get("selected_message_id") != follow_up_message_id:
            raise RuntimeError(
                json.dumps(
                    {
                        "error": "graft follow-up message was not readable by operator",
                        "follow_up_message_id": follow_up_message_id,
                        "read_payload": follow_up_read,
                    },
                    indent=2,
                )
            )
        if daemon_pid is None or not process_is_alive(daemon_pid):
            raise RuntimeError("graft same-host smoke daemon was not alive during host-lane validation")

        print(
            json.dumps(
                {
                    "status": "passed",
                    "note": "same-host atm-graft host registered, consumed advisory nudge, completed read/ack/send over the shared daemon contract, and the operator read the graft follow-up successfully",
                    "initial_message_id": initial_send["message_id"],
                    "follow_up_message_id": follow_up_message_id,
                    "daemon_pids_before": daemon_pids_before,
                    "daemon_pids_during": daemon_pids_during,
                    "example": example_payload,
                }
            )
        )
        return 0
    finally:
        if example_proc is not None and example_proc.poll() is None:
            example_proc.terminate()
            try:
                example_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                example_proc.kill()
                example_proc.wait(timeout=5)
        terminate_process(daemon_pid)
        wait_for_process_exit(daemon_pid)
        daemon_pids_after = count_atm_daemon_processes()
        assert_no_process_leak(daemon_pids_before, daemon_pids_after)
        shutil.rmtree(fixture.root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())

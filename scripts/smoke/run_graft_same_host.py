#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from daemon_lifecycle import (
    assert_no_process_leak,
    count_atm_daemon_processes,
    process_is_alive,
    require_clean_host_daemon_state,
    terminate_process,
    wait_for_process_exit,
)
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


def smoke_example_binary(root: Path, name: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    binary = root / "target" / "debug" / "examples" / f"{name}{suffix}"
    if not binary.is_file():
        raise RuntimeError(f"same-host graft smoke requires built example binary at {binary}")
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
    if os.name == "nt":
        capture_dir = cwd / ".atm-smoke-captures"
        capture_dir.mkdir(exist_ok=True)
        stem = f"{os.getpid()}-{time.monotonic_ns()}"
        stdout_path = capture_dir / f"{stem}.stdout"
        stderr_path = capture_dir / f"{stem}.stderr"
        with stdout_path.open("w+", encoding="utf-8", errors="replace") as stdout_file, stderr_path.open(
            "w+", encoding="utf-8", errors="replace"
        ) as stderr_file:
            completed = subprocess.run(
                command,
                cwd=cwd,
                env=env,
                stdout=stdout_file,
                stderr=stderr_file,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
                timeout=30,
                input=stdin,
            )
            stdout_file.seek(0)
            stderr_file.seek(0)
            stdout = stdout_file.read()
            stderr = stderr_file.read()
        return {
            "command": command,
            "cwd": str(cwd),
            "exit_code": completed.returncode,
            "stdout": stdout.strip(),
            "stderr": stderr.strip(),
        }
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


def current_fixture_daemon_pid(root: Path, env: dict[str, str], cwd: Path) -> int | None:
    payload = parse_json_output(run_atm(root, env, cwd, "doctor", "--json"))
    owner_pid = payload.get("runtime_status", {}).get("singleton_owner_pid")
    if isinstance(owner_pid, int):
        return owner_pid
    if isinstance(owner_pid, str) and owner_pid.isdigit():
        return int(owner_pid)
    return None


def wait_for_file(path: Path, timeout_seconds: float = 30.0) -> None:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for graft ready file: {path}")


def isolated_daemon_baseline() -> list[int]:
    """Refuse ambient daemons and retain the exact process baseline for cleanup."""
    require_clean_host_daemon_state(smoke_label="graft same-host smoke")
    baseline = count_atm_daemon_processes()
    if baseline:
        raise RuntimeError(
            "graft same-host smoke found an ambient atm-daemon after clean-host preflight; "
            "refusing to attach to or terminate it"
        )
    return baseline


def ensure_member(root: Path, env: dict[str, str], workspace_dir: Path, team: str, member: str) -> None:
    completed = run_atm_result(
        root,
        env,
        workspace_dir,
        "teams",
        "add-member",
        team,
        member,
        "--home-dir",
        str(workspace_dir),
        "--json",
    )
    if completed["exit_code"] == 0:
        return
    stderr = str(completed.get("stderr", ""))
    if "already exists in team" in stderr:
        return
    raise RuntimeError(json.dumps(completed, indent=2))


def update_member_harness(
    root: Path,
    env: dict[str, str],
    workspace_dir: Path,
    team: str,
    member: str,
    harness: str,
) -> None:
    completed = run_atm_result(
        root,
        env,
        workspace_dir,
        "teams",
        "update-member",
        team,
        member,
        "--harness",
        harness,
        "--agent-type",
        "worker",
        "--json",
    )
    if completed["exit_code"] != 0:
        raise RuntimeError(json.dumps(completed, indent=2))


def main() -> int:
    root = repo_root()
    daemon_pids_before = isolated_daemon_baseline()
    ensure_debug_binaries(root)

    unique = next(tempfile._get_candidate_names()).replace("_", "")[:8]
    team_name = f"g{unique}"
    operator = f"o{unique}"
    graft_agent = f"a{unique}"
    fixture = create_clean_room_fixture(
        prefix="z21g.",
        team_name=team_name,
        operator=operator,
        recipient=graft_agent,
    )
    daemon_pid: int | None = None
    example_proc: subprocess.Popen[str] | None = None
    example_stdout_handle = None
    example_stderr_handle = None
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
        update_member_harness(
            root,
            operator_env,
            fixture.workspace_dir,
            team_name,
            graft_agent,
            "codex-cli",
        )

        ready_file = fixture.root / "graft-ready.txt"
        example_command = [
            str(smoke_example_binary(root, "smoke_same_host")),
            str(fixture.workspace_dir),
            team_name,
            graft_agent,
            f"{operator}@{team_name}",
            "graft smoke request",
            operator,
            str(ready_file),
        ]
        example_stdout_path = fixture.root / "example.stdout.log"
        example_stderr_path = fixture.root / "example.stderr.log"
        example_stdout_handle = example_stdout_path.open("w", encoding="utf-8")
        example_stderr_handle = example_stderr_path.open("w", encoding="utf-8")
        example_proc = subprocess.Popen(
            example_command,
            cwd=root,
            env=graft_env,
            stdout=example_stdout_handle,
            stderr=example_stderr_handle,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        wait_for_file(ready_file)
        daemon_pid = current_fixture_daemon_pid(root, operator_env, fixture.workspace_dir)
        if daemon_pid is None:
            raise RuntimeError(
                json.dumps(
                    {
                        "error": "graft same-host smoke expected a fixture-scoped daemon owner pid after graft host activation",
                    },
                    indent=2,
                )
            )
        daemon_pids_during = [daemon_pid]

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

        example_proc.wait(timeout=60)
        stdout = (
            example_stdout_path.read_text(encoding="utf-8")
            if example_stdout_path.is_file()
            else ""
        )
        stderr = (
            example_stderr_path.read_text(encoding="utf-8")
            if example_stderr_path.is_file()
            else ""
        )
        if example_proc.returncode != 0:
            raise RuntimeError(
                json.dumps(
                    {
                        "command": example_command,
                        "exit_code": example_proc.returncode,
                        "stdout": stdout.strip(),
                        "stderr": stderr.strip(),
                        "initial_send": initial_send,
                        "daemon_pids_during": daemon_pids_during,
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
                    "daemon_pids_during": daemon_pids_during,
                    "example": example_payload,
                }
            )
        )
        return 0
    finally:
        if example_stdout_handle is not None:
            example_stdout_handle.close()
        if example_stderr_handle is not None:
            example_stderr_handle.close()
        if example_proc is not None and example_proc.poll() is None:
            example_proc.terminate()
            try:
                example_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                example_proc.kill()
                example_proc.wait(timeout=5)
        terminate_process(daemon_pid)
        wait_for_process_exit(
            daemon_pid,
            process_label="graft same-host smoke daemon",
        )
        assert_no_process_leak(
            daemon_pids_before,
            count_atm_daemon_processes(),
            smoke_label="graft same-host smoke",
        )
        shutil.rmtree(fixture.root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())

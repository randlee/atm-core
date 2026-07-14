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

from fixtures import create_shared_host_fixture_pair, repo_root, smoke_env

AF1_REQUIRED_MARKERS = (
    "require_clean_host_daemon_state",
    "count_atm_daemon_processes",
    "assert_no_process_leak",
)


def debug_binary(root: Path, name: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    return root / "target" / "debug" / f"{name}{suffix}"


def smoke_binary(root: Path, name: str) -> Path:
    """Resolve the explicitly selected installed artifact, or CI's debug pair."""
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
        ["cargo", "build", "-p", "agent-team-mail", "-p", "atm-daemon"],
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


def run_atm(root: Path, env: dict[str, str], cwd: Path, *args: str) -> str:
    with tempfile.NamedTemporaryFile(mode="w+", encoding="utf-8") as stdout_handle, tempfile.NamedTemporaryFile(
        mode="w+",
        encoding="utf-8",
    ) as stderr_handle:
        command = [str(smoke_binary(root, "atm")), *args]
        try:
            completed = subprocess.run(
                command,
                cwd=cwd,
                env=env,
                stdout=stdout_handle,
                stderr=stderr_handle,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
                timeout=30,
            )
        except subprocess.TimeoutExpired as error:
            stdout_handle.seek(0)
            stderr_handle.seek(0)
            raise RuntimeError(
                json.dumps(
                    {
                        "command": command,
                        "cwd": str(cwd),
                        "exit_code": "timeout",
                        "timeout_seconds": error.timeout,
                        "stdout": stdout_handle.read().strip(),
                        "stderr": stderr_handle.read().strip(),
                    },
                    indent=2,
                )
            ) from error
        stdout_handle.seek(0)
        stderr_handle.seek(0)
        stdout = stdout_handle.read()
        stderr = stderr_handle.read()
    if completed.returncode != 0:
        raise RuntimeError(
            json.dumps(
                {
                    "command": command,
                    "cwd": str(cwd),
                    "exit_code": completed.returncode,
                    "stdout": stdout.strip(),
                    "stderr": stderr.strip(),
                },
                indent=2,
            )
        )
    return stdout


def run_atm_raw(
    root: Path,
    env: dict[str, str],
    cwd: Path,
    *args: str,
) -> subprocess.CompletedProcess[str]:
    command = [str(smoke_binary(root, "atm")), *args]
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
        timeout=30,
    )


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
            f"shared-host smoke daemon pid {pid} did not exit within {timeout_seconds}s"
        )


def require_clean_host_daemon_state() -> None:
    """Keep the shared-host smoke from attaching to a developer's daemon.

    AF-1 deliberately has no alternate runtime-root escape hatch. The smoke
    therefore must run under an isolated OS user/host with no pre-existing ATM
    daemon, rather than redirecting HOME or killing a process it did not start.
    """
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
            "shared-host smoke requires an isolated OS user with no existing "
            "atm-daemon; refusing to attach to or terminate an ambient daemon"
        )


def verify_af1_preflight_contract() -> None:
    source = Path(__file__).read_text(encoding="utf-8")
    missing = [marker for marker in AF1_REQUIRED_MARKERS if marker not in source]
    if missing:
        raise RuntimeError(
            "shared-host smoke refuses to start because AF-1 preflight/cleanup "
            f"markers are absent: {', '.join(missing)}"
        )


def verify_removed_cli_flags_stay_rejected(root: Path, env: dict[str, str], cwd: Path) -> None:
    removed_flag_commands = [
        ("send", "nobody@test-team", "probe", "--from", "legacy-sender"),
        ("ack", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "probe", "--as", "legacy-sender"),
    ]
    for command in removed_flag_commands:
        completed = run_atm_raw(root, env, cwd, *command)
        if completed.returncode == 0:
            raise RuntimeError(
                f"shared-host smoke preflight expected removed CLI syntax to fail but it succeeded: {' '.join(command)}"
            )


def assert_no_process_leak(before: list[int], after: list[int]) -> None:
    leaked = sorted(set(after) - set(before))
    if leaked:
        raise RuntimeError(
            f"shared-host smoke detected leaked atm-daemon pid(s): {', '.join(str(pid) for pid in leaked)}"
        )


def maybe_inject_leaked_child() -> subprocess.Popen[str] | None:
    if os.environ.get("ATM_SMOKE_INJECT_LEAK_CHILD") != "1":
        return None
    return subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(60)"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )


def main() -> int:
    root = repo_root()
    verify_af1_preflight_contract()
    daemon_pids_before = count_atm_daemon_processes()
    require_clean_host_daemon_state()
    ensure_debug_binaries(root)
    shared_host_fixture_pair = create_shared_host_fixture_pair(
        prefix="z21s.",
        team_name_a="z21-shared-a",
        team_name_b="z21-shared-b",
        operator_a="z21-shared-operator-a",
        operator_b="z21-shared-operator-b",
        recipient_a="z21-shared-recipient-a",
        recipient_b="z21-shared-recipient-b",
    )
    shared_a = shared_host_fixture_pair.workspace_a
    shared_b = shared_host_fixture_pair.workspace_b
    shared_env_a = smoke_env(shared_a, identity=shared_a.operator, root=root)
    shared_env_b = smoke_env(shared_b, identity=shared_b.operator, root=root)
    debug_daemon = str(smoke_binary(root, "atm-daemon"))
    shared_env_a["ATM_DAEMON_BIN"] = debug_daemon
    shared_env_b["ATM_DAEMON_BIN"] = debug_daemon
    shared_daemon_pid: int | None = None
    injected_leak_child = maybe_inject_leaked_child()
    try:
        verify_removed_cli_flags_stay_rejected(root, shared_env_a, shared_a.workspace_dir)
        # `doctor` is the first CLI invocation that may auto-start a daemon.
        # Recheck here so no process can appear during the build/fixture setup
        # window after the initial fail-closed check.
        require_clean_host_daemon_state()
        shared_doctor_a = parse_json_output(
            run_atm(root, shared_env_a, shared_a.workspace_dir, "doctor", "--json")
        )
        shared_doctor_b = parse_json_output(
            run_atm(root, shared_env_b, shared_b.workspace_dir, "doctor", "--json")
        )
        shared_pid_a = shared_doctor_a.get("runtime_status", {}).get("singleton_owner_pid")
        shared_pid_b = shared_doctor_b.get("runtime_status", {}).get("singleton_owner_pid")
        shared_daemon_pid = int(shared_pid_a) if shared_pid_a is not None else None
        daemon_pids_during = count_atm_daemon_processes()

        for fixture_item, env_item in ((shared_a, shared_env_a), (shared_b, shared_env_b)):
            run_atm(
                root,
                env_item,
                fixture_item.workspace_dir,
                "teams",
                "add-member",
                fixture_item.team_name,
                fixture_item.operator,
                "--json",
            )
            run_atm(
                root,
                env_item,
                fixture_item.workspace_dir,
                "teams",
                "add-member",
                fixture_item.team_name,
                fixture_item.recipient,
                "--json",
            )

        def run_send(fixture_item: object, env_item: dict[str, str], body: str) -> dict[str, object]:
            target = f"{fixture_item.recipient}@{fixture_item.team_name}"  # type: ignore[attr-defined]
            return parse_json_output(
                run_atm(
                    root,
                    env_item,
                    fixture_item.workspace_dir,  # type: ignore[attr-defined]
                    "send",
                    target,
                    body,
                    "--requires-ack",
                    "--json",
                )
            )

        shared_send_a = run_send(shared_a, shared_env_a, "shared-host message from workspace A")
        shared_send_b = run_send(shared_b, shared_env_b, "shared-host message from workspace B")

        shared_message_id_a = str(shared_send_a["message_id"])
        shared_message_id_b = str(shared_send_b["message_id"])

        def read_and_ack(
            fixture_item: object,
            env_item: dict[str, str],
            message_id: str,
            ack_body: str,
        ) -> dict[str, object]:
            recipient_env = env_item.copy()
            recipient_env["ATM_IDENTITY"] = fixture_item.recipient  # type: ignore[attr-defined]
            read_payload = parse_json_output(
                run_atm(
                    root,
                    recipient_env,
                    fixture_item.workspace_dir,  # type: ignore[attr-defined]
                    "read",
                    "--team",
                    fixture_item.team_name,  # type: ignore[attr-defined]
                    "--all",
                    "--message-id",
                    message_id,
                    "--json",
                )
            )
            ack_payload = parse_json_output(
                run_atm(
                    root,
                    recipient_env,
                    fixture_item.workspace_dir,  # type: ignore[attr-defined]
                    "ack",
                    message_id,
                    ack_body,
                    "--team",
                    fixture_item.team_name,  # type: ignore[attr-defined]
                    "--json",
                )
            )
            return {"read": read_payload, "ack": ack_payload}

        shared_read_ack_a = read_and_ack(
            shared_a,
            shared_env_a,
            shared_message_id_a,
            "shared-host ack A",
        )
        shared_read_ack_b = read_and_ack(
            shared_b,
            shared_env_b,
            shared_message_id_b,
            "shared-host ack B",
        )

        shared_list_a = parse_json_output(
            run_atm(
                root,
                shared_env_a,
                shared_a.workspace_dir,
                "list",
                "--team",
                shared_a.team_name,
                "--json",
            )
        )
        shared_list_b = parse_json_output(
            run_atm(
                root,
                shared_env_b,
                shared_b.workspace_dir,
                "list",
                "--team",
                shared_b.team_name,
                "--json",
            )
        )
        shared_log_snapshot_a = parse_json_output(
            run_atm(root, shared_env_a, shared_a.workspace_dir, "log", "snapshot", "--json")
        )
        shared_records_a = json.dumps(shared_list_a)
        shared_records_b = json.dumps(shared_list_b)
        shared_host_ok = (
            shared_doctor_a.get("summary", {}).get("status") == "healthy"
            and shared_doctor_b.get("summary", {}).get("status") == "healthy"
            and shared_pid_a is not None
            and shared_pid_a == shared_pid_b
            and shared_send_a.get("outcome") == "sent"
            and shared_send_b.get("outcome") == "sent"
            and shared_read_ack_a["read"].get("selected_message_id") == shared_message_id_a
            and shared_read_ack_b["read"].get("selected_message_id") == shared_message_id_b
            and shared_read_ack_a["ack"].get("message_id") == shared_message_id_a
            and shared_read_ack_b["ack"].get("message_id") == shared_message_id_b
            and shared_message_id_b not in shared_records_a
            and shared_message_id_a not in shared_records_b
            and isinstance(shared_log_snapshot_a.get("records"), list)
            and process_is_alive(int(shared_pid_a))
        )
        if shared_host_ok:
            print(
                json.dumps(
                    {
                        "status": "passed",
                        "note": "two workspaces with one shared ATM_HOME daemon/database/log root handled raw CLI send/read/ack traffic without cross-workspace leakage",
                        "daemon_pids_before": daemon_pids_before,
                        "daemon_pids_during": daemon_pids_during,
                    }
                )
            )
            return 0

        print(
            json.dumps(
                {
                    "status": "failed",
                    "doctor_a": shared_doctor_a,
                    "doctor_b": shared_doctor_b,
                    "send_a": shared_send_a,
                    "send_b": shared_send_b,
                    "read_ack_a": shared_read_ack_a,
                    "read_ack_b": shared_read_ack_b,
                    "list_a": shared_list_a,
                    "list_b": shared_list_b,
                    "log_snapshot_a": shared_log_snapshot_a,
                    "daemon_pids_before": daemon_pids_before,
                    "daemon_pids_during": daemon_pids_during,
                },
                indent=2,
            )
        )
        return 1
    finally:
        terminate_process(shared_daemon_pid)
        wait_for_process_exit(shared_daemon_pid)
        daemon_pids_after = count_atm_daemon_processes()
        if injected_leak_child is not None and injected_leak_child.poll() is None:
            injected_leak_child.terminate()
            injected_leak_child.wait(timeout=5)
            raise RuntimeError(
                "shared-host smoke leak fault injection detected a surviving child process"
            )
        assert_no_process_leak(daemon_pids_before, daemon_pids_after)
        shutil.rmtree(shared_host_fixture_pair.root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Run the AQ1.9 Hermes graft restart matrix on one dedicated host.

The runner owns every process and temporary configuration root it creates. The
daemon runtime and database are intentionally OS-account scoped, so the runner
fails closed when an ambient daemon is present. It uses a separate Python
receiver worker so the crash row can kill the receiver while leaving the
Tokio/Axum daemon and sender alive. The same command is valid on the local
loopback host and on m5; ``--host`` is recorded in the evidence.
"""

from __future__ import annotations

import argparse
from collections import deque
import json
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from typing import Any, Callable


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq1-9-hermes"
SENDER = "aq1-9-sender"
RECEIVER = "aq1-9-receiver"
LEASE_REFRESH_INTERVAL_SECONDS = 1.0
CRASH_RECOVERY_LIMIT_SECONDS = 1.5
EVENT_TIMEOUT_SECONDS = 15.0
HOST_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="local", help="evidence host label, for example local or m5")
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=None,
        help="directory for the JSON and Markdown evidence files",
    )
    parser.add_argument(
        "--daemon",
        type=Path,
        default=Path(os.environ.get("ATM_DAEMON_BIN", ROOT / "target" / "release" / "atm-daemon")),
        help="owned atm-daemon binary",
    )
    parser.add_argument(
        "--atm",
        type=Path,
        default=Path(os.environ.get("ATM_BIN", ROOT / "target" / "release" / "atm")),
        help="matched atm CLI binary",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=EVENT_TIMEOUT_SECONDS,
        help="per-row readiness and delivery timeout in seconds",
    )
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--workspace-root", type=Path, help=argparse.SUPPRESS)
    return parser.parse_args()


class OutputCapture:
    """Drain a child process and retain a bounded diagnostic transcript."""

    def __init__(self, process: subprocess.Popen[str]) -> None:
        if process.stdout is None or process.stderr is None:
            raise RuntimeError("matrix child output was not captured")
        self.events: queue.Queue[str | None] = queue.Queue()
        self.lines: deque[str] = deque(maxlen=80)
        self._threads = (
            threading.Thread(target=self._read_stdout, args=(process.stdout,), daemon=True),
            threading.Thread(target=self._read_stderr, args=(process.stderr,), daemon=True),
        )
        for thread in self._threads:
            thread.start()

    def _read_stdout(self, stream: Any) -> None:
        for line in stream:
            clean = line.rstrip("\n")
            self.lines.append(clean)
            self.events.put(clean)
        self.events.put(None)

    def _read_stderr(self, stream: Any) -> None:
        for line in stream:
            self.lines.append(line.rstrip("\n"))

    def wait_for(self, predicate: Callable[[str], bool], timeout: float) -> str:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                line = self.events.get(timeout=min(0.1, deadline - time.monotonic()))
            except queue.Empty:
                continue
            if line is None:
                raise RuntimeError("matrix child exited before its expected event")
            if predicate(line):
                return line
        raise TimeoutError("matrix child did not emit its expected event")

    def tail(self) -> list[str]:
        return list(self.lines)


def process_alive(process: subprocess.Popen[str]) -> bool:
    return process.poll() is None


def ambient_daemon_pids() -> list[int]:
    """Return same-account daemon processes before touching host state."""
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
            columns = [item.strip('"') for item in line.split('","')]
            if columns and columns[0].lower() == "atm-daemon.exe" and len(columns) > 1 and columns[1].isdigit():
                pids.append(int(columns[1]))
        return pids
    completed = subprocess.run(
        ["ps", "-axo", "uid=,pid=,command="],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError("could not inspect same-account daemon processes")
    owner_uid = os.getuid()
    pids = []
    for line in completed.stdout.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) != 3 or not parts[0].isdigit() or not parts[1].isdigit():
            continue
        executable = parts[2].split(None, 1)[0]
        if int(parts[0]) == owner_uid and Path(executable).name == "atm-daemon":
            pids.append(int(parts[1]))
    return pids


def require_clean_host() -> None:
    pids = ambient_daemon_pids()
    if pids:
        rendered = ", ".join(str(pid) for pid in pids)
        raise SystemExit(
            "AQ1.9 live matrix requires a dedicated OS account with no ambient "
            f"atm-daemon; found pid(s) {rendered}. The daemon runtime/database "
            "scope intentionally ignores ATM_HOME."
        )


def stop_process(process: subprocess.Popen[str] | None, *, crash: bool = False) -> None:
    if process is None or process.poll() is not None:
        return
    if crash:
        process.kill()
    else:
        process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def worker_main(args: argparse.Namespace) -> int:
    if args.workspace_root is None:
        raise SystemExit("worker requires --workspace-root")
    try:
        import atm_graft
    except ImportError as error:
        raise SystemExit("atm_graft is not installed; install the built wheel first") from error

    stop = threading.Event()

    def request_stop(_signum: int, _frame: Any) -> None:
        stop.set()

    if hasattr(signal, "SIGTERM"):
        signal.signal(signal.SIGTERM, request_stop)
    agent = os.environ["ATM_IDENTITY"]
    team = os.environ["ATM_TEAM"]
    caller = atm_graft.PyAgentAddress(agent, team, None)
    options = atm_graft.PyGraftSessionOptions(str(args.workspace_root), agent, team)
    session = atm_graft.PyGraftSession(caller)

    def on_nudge(nudge: Any) -> None:
        print(
            json.dumps(
                {
                    "kind": "nudge",
                    "at_ns": time.time_ns(),
                    "message_id": nudge.message_id,
                    "body": nudge.body,
                },
                sort_keys=True,
            ),
            flush=True,
        )

    try:
        session.activate_receiver(options, on_nudge)
        print(json.dumps({"kind": "ready", "at_ns": time.time_ns()}), flush=True)
        while not stop.wait(0.1):
            pass
    finally:
        session.close()
    return 0


class OwnedDaemon:
    def __init__(self, binary: Path, env: dict[str, str], timeout: float) -> None:
        self.binary = binary
        self.env = env
        self.timeout = timeout
        self.process: subprocess.Popen[str] | None = None
        self.output: OutputCapture | None = None

    def start(self) -> dict[str, Any]:
        if self.process is not None and process_alive(self.process):
            raise RuntimeError("owned daemon is already running")
        self.process = subprocess.Popen(
            [str(self.binary), "--peer-wire-security", "mutual-tls"],
            cwd=ROOT,
            env={**self.env, "ATM_DAEMON_READY_STDOUT": "1"},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        self.output = OutputCapture(self.process)
        assert self.output is not None
        self.output.wait_for(lambda line: line.strip() == "ATM_DAEMON_READY", self.timeout)
        return {"pid": self.process.pid, "output_tail": self.output.tail()}

    def stop(self) -> dict[str, Any]:
        process = self.process
        output = self.output
        stop_process(process)
        return {
            "pid": process.pid if process is not None else None,
            "returncode": process.returncode if process is not None else None,
            "output_tail": output.tail() if output is not None else [],
        }


class ReceiverWorker:
    def __init__(self, script: Path, workspace_root: Path, env: dict[str, str], timeout: float) -> None:
        self.script = script
        self.workspace_root = workspace_root
        self.env = env
        self.timeout = timeout
        self.process: subprocess.Popen[str] | None = None
        self.output: OutputCapture | None = None

    def start(self) -> dict[str, Any]:
        self.process = subprocess.Popen(
            [
                sys.executable,
                str(self.script),
                "--worker",
                "--workspace-root",
                str(self.workspace_root),
            ],
            cwd=ROOT,
            env={**self.env, "ATM_IDENTITY": RECEIVER, "PYTHONUNBUFFERED": "1"},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        self.output = OutputCapture(self.process)
        assert self.output is not None

        def ready_event(line: str) -> bool:
            try:
                return json.loads(line).get("kind") == "ready"
            except json.JSONDecodeError:
                return False

        self.output.wait_for(ready_event, self.timeout)
        return {"pid": self.process.pid}

    def wait_for_nudge(self, marker: str, timeout: float) -> dict[str, Any]:
        if self.output is None:
            raise RuntimeError("receiver worker is not running")

        def matching_event(line: str) -> bool:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                return False
            return event.get("kind") == "nudge" and marker in event.get("body", "")

        line = self.output.wait_for(matching_event, timeout)
        return json.loads(line)

    def stop(self, *, crash: bool = False) -> dict[str, Any]:
        process = self.process
        output = self.output
        stop_process(process, crash=crash)
        return {
            "pid": process.pid if process is not None else None,
            "returncode": process.returncode if process is not None else None,
            "crash": crash,
            "output_tail": output.tail() if output is not None else [],
        }


def fixture_environment(root: Path, atm: Path) -> tuple[dict[str, str], Path]:
    workspace = root / "workspace"
    home = root / "home"
    atm_home = root / "atm-home"
    logs = root / "logs"
    workspace.mkdir(parents=True)
    home.mkdir()
    atm_home.mkdir()
    logs.mkdir()
    (workspace / ".atm.toml").write_text(f'[atm]\ndefault_team = "{TEAM}"\n', encoding="utf-8")
    temp_dir = root / "tmp"
    temp_dir.mkdir()
    environment = {
        **os.environ,
        "HOME": str(home),
        "ATM_HOME": str(atm_home),
        "ATM_CONFIG_HOME": str(atm_home),
        "ATM_WORKSPACE_ROOT": str(workspace),
        "ATM_TEAM": TEAM,
        "ATM_IDENTITY": SENDER,
        "ATM_DAEMON_BIN": str(atm.parent / "atm-daemon"),
        "ATM_LOG": "debug",
        "ATM_LOG_DIR": str(logs),
        "TMPDIR": str(temp_dir),
        "TMP": str(temp_dir),
        "TEMP": str(temp_dir),
    }
    return environment, workspace


def add_roster_member(atm: Path, env: dict[str, str], home: Path, member: str) -> None:
    completed = subprocess.run(
        [str(atm), "teams", "add-member", TEAM, member, "--home-dir", str(home), "--json"],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"could not add {member} to the isolated roster: {detail}")


def remove_roster_member(
    atm: Path,
    env: dict[str, str],
    member: str,
    *,
    caller: str = RECEIVER,
) -> None:
    subprocess.run(
        [str(atm), "teams", "remove-member", TEAM, member, "--json"],
        cwd=ROOT,
        env={**env, "ATM_IDENTITY": caller},
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def doctor(atm: Path, env: dict[str, str]) -> dict[str, Any]:
    completed = subprocess.run(
        [str(atm), "doctor", "--json"],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError:
        payload = {"exit_code": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr}
    return payload


def run_scenario(args: argparse.Namespace, row: str, action: str) -> dict[str, Any]:
    started_at = time.time_ns()
    with tempfile.TemporaryDirectory(prefix=f"aq1-9-{row}-") as temporary:
        root = Path(temporary)
        env, workspace = fixture_environment(root, args.atm)
        home = Path(env["HOME"])
        daemon = OwnedDaemon(args.daemon, env, args.timeout)
        receiver: ReceiverWorker | None = None
        sender_session: Any | None = None
        roster_members: list[str] = []
        record: dict[str, Any] = {
            "id": row,
            "action": action,
            "started_at_ns": started_at,
            "status": "fail",
            "host": args.host,
        }
        try:
            add_roster_member(args.atm, env, home, SENDER)
            roster_members.append(SENDER)
            add_roster_member(args.atm, env, home, RECEIVER)
            roster_members.append(RECEIVER)
            daemon_start = daemon.start()
            record["daemon_before"] = daemon_start
            record["doctor_before"] = doctor(args.atm, env)
            receiver = ReceiverWorker(Path(__file__).resolve(), workspace, env, args.timeout)
            record["receiver_before"] = receiver.start()
            os.environ.update({key: env[key] for key in ("HOME", "ATM_HOME", "ATM_CONFIG_HOME", "ATM_WORKSPACE_ROOT", "ATM_TEAM", "ATM_IDENTITY", "ATM_DAEMON_BIN", "ATM_LOG_DIR", "TMPDIR", "TMP", "TEMP")})
            import atm_graft

            sender_session = atm_graft.PyGraftSession(atm_graft.PyAgentAddress(SENDER, TEAM, None))
            receiver_address = atm_graft.PyAgentAddress(RECEIVER, TEAM, None)
            if action == "daemon_restart":
                record["restart_at_ns"] = time.time_ns()
                record["daemon_stop"] = daemon.stop()
                record["daemon_after"] = daemon.start()
            else:
                crash = action == "receiver_crash_within_window"
                record["restart_at_ns"] = time.time_ns()
                record["receiver_stop"] = receiver.stop(crash=crash)
                receiver = ReceiverWorker(Path(__file__).resolve(), workspace, env, args.timeout)
                record["receiver_after"] = receiver.start()
            marker = f"aq1-9-{row}-{uuid.uuid4()}"
            send_started = time.time_ns()
            sent = sender_session.send(receiver_address, marker, False)
            nudge = receiver.wait_for_nudge(marker, args.timeout)
            delivered_at = int(nudge["at_ns"])
            record.update(
                {
                    "marker": marker,
                    "message_id": str(sent.message_id),
                    "sent_at_ns": send_started,
                    "delivered_at_ns": delivered_at,
                    "delivery_latency_ms": round((delivered_at - send_started) / 1_000_000, 3),
                    "nudge": nudge,
                    "receiver_transcript": receiver.output.tail() if receiver.output is not None else [],
                    "daemon_transcript": daemon.output.tail() if daemon.output is not None else [],
                    "doctor_after": doctor(args.atm, env),
                    "status": "pass",
                }
            )
            if action == "receiver_crash_within_window":
                recovery_ms = (delivered_at - int(record["restart_at_ns"])) / 1_000_000
                record["crash_recovery_ms"] = round(recovery_ms, 3)
                record["within_one_refresh_tick"] = recovery_ms <= CRASH_RECOVERY_LIMIT_SECONDS * 1000
                if not record["within_one_refresh_tick"]:
                    record["status"] = "fail"
                    record["error"] = "successor delivery exceeded the one-refresh-tick recovery bound"
        except Exception as error:  # noqa: BLE001 - evidence must retain the row failure
            record["error"] = f"{type(error).__name__}: {error}"
        finally:
            if sender_session is not None:
                try:
                    sender_session.close()
                except Exception:
                    pass
            if receiver is not None:
                receiver.stop()
            record["daemon_cleanup"] = daemon.stop()
            if RECEIVER in roster_members:
                remove_roster_member(args.atm, env, SENDER)
                remove_roster_member(args.atm, env, RECEIVER)
            elif SENDER in roster_members:
                remove_roster_member(args.atm, env, SENDER, caller=SENDER)
            record["finished_at_ns"] = time.time_ns()
        return record


def write_evidence(args: argparse.Namespace, records: list[dict[str, Any]]) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ1.9"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    json_path = evidence_dir / f"restart-matrix-{args.host}.json"
    markdown_path = evidence_dir / f"restart-matrix-{args.host}.md"
    payload = {
        "schema_version": 1,
        "sprint": "AQ1.9",
        "host": args.host,
        "commit": subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip(),
        "lease_refresh_interval_seconds": LEASE_REFRESH_INTERVAL_SECONDS,
        "records": records,
    }
    json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    lines = [
        "# AQ1.9 Hermes ATM restart matrix",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{payload['commit']}`",
        "",
        "| Row | Status | Delivery latency | Evidence |",
        "| --- | --- | ---: | --- |",
    ]
    for record in records:
        latency = record.get("delivery_latency_ms", "—")
        lines.append(f"| {record['id']} | {record['status'].upper()} | {latency} ms | `{json_path.name}` |")
    lines.extend(
        [
            "",
            "The daemon restart row keeps the receiver worker alive. The receiver restart row performs a clean close and immediate replacement. The crash row uses SIGKILL and starts the successor inside the active lease window; its one-refresh-tick assertion is recorded in JSON.",
            "",
            "The m5 live run must be executed on m5; no remote result is inferred from this local artifact.",
        ]
    )
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    if not HOST_RE.fullmatch(args.host):
        raise SystemExit("--host must contain only letters, numbers, '.', '_', or '-'")
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    if args.worker:
        return worker_main(args)
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    require_clean_host()
    records = [
        run_scenario(args, "daemon-restart-live-receiver", "daemon_restart"),
        run_scenario(args, "receiver-restart-live-daemon", "receiver_restart"),
        run_scenario(args, "receiver-crash-within-window", "receiver_crash_within_window"),
    ]
    json_path, markdown_path = write_evidence(args, records)
    passed = all(record.get("status") == "pass" for record in records)
    print(f"{'PASS' if passed else 'FAIL'} restart matrix evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())

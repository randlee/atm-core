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
import contextlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import uuid
from typing import Any, Callable, Iterator


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq1-9-hermes"
SENDER = "aq1-9-sender"
RECEIVER = "aq1-9-receiver"
LEASE_REFRESH_INTERVAL_SECONDS = 1.0
# AC2 (sprint-AQ1-9) / AQ1.6 AC5: after a SIGKILLed receiver, the successor's
# *bind-time* registration displaces the stale lease -- zero refresh ticks.
# That is a product observable (`atm doctor --json` `graft_receivers`), and it
# is what the crash row asserts on. Wall-clock recovery is recorded as a
# diagnostic only: `restart_at_ns` is stamped before the kill *and* before the
# successor worker's fresh CPython + `import atm_graft` spawn, so a wall-clock
# bound charges interpreter start-up (observed 933 ms on a Windows CI runner
# vs 170-270 ms on linux/macOS) against the product. See
# docs/aq-closeout @ 9674f64b7 (merge b78c041f1): the product displaced the
# lease at +211 ms while the old one-tick bound tripped on the 933 ms spawn.
RFC3339_RE = re.compile(
    r"^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?"
    r"(Z|z|[+-]\d{2}:?\d{2})$"
)
EVENT_TIMEOUT_SECONDS = 15.0
HOST_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}")
# `atm_graft`'s native PyO3 session runs in this process and reads its
# configuration from the process environment directly -- unlike every other
# child this runner starts, there is no per-call `env=` argument for it.
NATIVE_SESSION_ENV_KEYS = (
    "HOME",
    "ATM_HOME",
    "ATM_CONFIG_HOME",
    "ATM_WORKSPACE_ROOT",
    "ATM_TEAM",
    "ATM_IDENTITY",
    "ATM_DAEMON_BIN",
    "ATM_LOG_DIR",
    "TMPDIR",
    "TMP",
    "TEMP",
)


@contextlib.contextmanager
def scoped_process_environment(overrides: dict[str, str]) -> Iterator[None]:
    """Temporarily apply `overrides` to the ambient process environment.

    Every subprocess this runner starts (the daemon, `atm`, the receiver
    worker) receives its own explicit `env=` dict and never reads ambient
    state. The one exception is the native `atm_graft` extension, which
    shares this process and therefore this process's environment -- there is
    no per-call env argument to give it instead. This context manager scopes
    that one unavoidable mutation to exactly the block that constructs and
    drives the native session, and restores every prior value (or absence)
    on exit, including on exceptions, so one restart-matrix row never leaks
    its fixture environment into a later row or into the caller's shell.
    """
    previous = {key: os.environ.get(key) for key in overrides}
    os.environ.update(overrides)
    try:
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def refresh_sender_after_daemon_restart(
    sender_session: Any,
    record: dict[str, Any],
    *,
    platform_name: str | None = None,
) -> None:
    """Refresh the native client after a Windows daemon restart.

    Windows local HTTP clients retain the daemon connection across a process
    restart, while the restarted daemon owns a new listener.  The graft
    binding exposes an explicit refresh operation; do not retry the write
    implicitly because a retry could duplicate a message.
    """
    if (platform_name or os.name) != "nt":
        return
    sender_session.reconnect()
    record["sender_reconnect"] = "windows-post-daemon-restart"


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


def daemon_launch_argv(binary: Path) -> list[str]:
    """Return the argv used to start the runner's owned, scratch-home daemon.

    The restart matrix verifies same-host graft receiver delivery, never
    cross-host peer replication, so it deliberately runs the daemon with
    `--peer-wire-security plaintext-test`: `mutual-tls` mode requires an
    enabled peer HTTPS interface and a local certificate identity
    (`crates/peer-tls/src/lib.rs`), which only exist in an operator's
    already-provisioned home directory, not in this runner's disposable
    `tempfile.TemporaryDirectory` home. Requiring real peer TLS material here
    would make the matrix unrunnable on a clean host (exactly what CI hit),
    without proving anything about the graft restart/crash-recovery
    behavior this sprint's ACs actually cover.
    """
    return [str(binary), "--peer-wire-security", "plaintext-test"]


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
            daemon_launch_argv(self.binary),
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

        ready_line = self.output.wait_for(ready_event, self.timeout)
        return {"pid": self.process.pid, "ready_at_ns": int(json.loads(ready_line)["at_ns"])}

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


def _remove_tree_tolerant(path: Path, *, attempts: int = 6, initial_delay: float = 0.15) -> str | None:
    """Best-effort recursive removal of one row's scratch directory.

    Mirrors `run_aq4_transfer_evidence.py`'s `_remove_tree_tolerant`. This
    row previously relied on `tempfile.TemporaryDirectory`'s automatic
    `__exit__` cleanup, whose exception (observed on Windows: WinError
    32/5 sharing violations against a directory this row's own `atm` /
    daemon / receiver-worker subprocesses -- already stopped and fully
    waited on above -- briefly still held a handle on) unwound straight
    past this row's own `try`/`except`/`finally` and the `return record`
    that used to sit inside it, discarding the already-computed row
    record entirely with no evidence written for that row at all.
    Retrying with backoff absorbs that transient lag; if the directory
    still will not budge, this reports a warning string instead of
    raising, so an OS-level cleanup race can never discard or crash a
    row's evidence. Returns `None` on success, or a description of the
    final failure.
    """
    delay = initial_delay
    last_error: OSError | None = None
    for attempt in range(attempts):
        try:
            shutil.rmtree(path)
            return None
        except FileNotFoundError:
            return None
        except OSError as error:
            last_error = error
            if attempt == attempts - 1:
                break
            time.sleep(delay)
            delay *= 2
    return f"could not remove {path} after {attempts} attempts: {last_error}"



def parse_rfc3339_ns(text: str) -> int:
    """Parse an RFC 3339 timestamp (up to nanosecond fraction) to epoch ns.

    `atm doctor --json` emits `registered_at` with nine fractional digits
    (`2026-08-28T21:12:08.776418200Z`), which `datetime.fromisoformat`
    does not accept on every supported Python; so parse it by hand.
    """
    match = RFC3339_RE.match(text.strip())
    if match is None:
        raise ValueError(f"not an RFC 3339 timestamp: {text!r}")
    year, month, day, hour, minute, second, fraction, zone = match.groups()
    from datetime import datetime, timedelta, timezone

    offset = timedelta(0)
    if zone not in ("Z", "z"):
        sign = 1 if zone[0] == "+" else -1
        digits = zone[1:].replace(":", "")
        offset = sign * timedelta(hours=int(digits[:2]), minutes=int(digits[2:]))
    moment = datetime(
        int(year), int(month), int(day), int(hour), int(minute), int(second), tzinfo=timezone(offset)
    )
    whole_ns = int(moment.timestamp()) * 1_000_000_000
    fraction_ns = int((fraction or "0").ljust(9, "0"))
    return whole_ns + fraction_ns


def receiver_leases(doctor_payload: dict[str, Any]) -> list[dict[str, Any]]:
    """Every `graft_receivers` lease for this row's receiver identity."""
    section = doctor_payload.get("graft_receivers") or {}
    receivers = section.get("receivers") if isinstance(section, dict) else None
    return [
        lease
        for lease in (receivers or [])
        if isinstance(lease, dict) and lease.get("team") == TEAM and lease.get("agent") == RECEIVER
    ]


def receiver_lease(doctor_payload: dict[str, Any]) -> dict[str, Any]:
    """The single pre-crash lease for this row's receiver identity."""
    leases = receiver_leases(doctor_payload)
    if len(leases) != 1:
        raise RuntimeError(f"expected exactly one pre-crash lease for {RECEIVER}, found {len(leases)}")
    return leases[0]


def successor_ready_at_ns(record: dict[str, Any]) -> int:
    """The successor's `ready` event timestamp from its transcript.

    Falls back to the `ready_at_ns` captured by `ReceiverWorker.start` when
    the transcript tail has already rotated the `ready` line out.
    """
    for line in record.get("receiver_transcript") or []:
        try:
            event = json.loads(line)
        except (TypeError, json.JSONDecodeError):
            continue
        if isinstance(event, dict) and event.get("kind") == "ready":
            return int(event["at_ns"])
    receiver_after = record.get("receiver_after") or {}
    if "ready_at_ns" in receiver_after:
        return int(receiver_after["ready_at_ns"])
    raise RuntimeError("successor receiver never reported a `ready` event")


def classify_crash_recovery(
    *,
    pre_crash_lease: dict[str, Any],
    leases_after: list[dict[str, Any]],
    successor_ready_at_ns: int,
    restart_at_ns: int,
    delivered_at_ns: int,
    delivery_matched: bool,
) -> dict[str, Any]:
    """Pure classification of the crash-within-window row (AQ1.6 AC5).

    The row passes on product observables only:

    1. `doctor_after` holds exactly one lease for the receiver identity;
    2. its `endpoint` differs from the pre-crash lease (a fresh bind, not
       the stale lease surviving);
    3. its `registered_at` is at or before the successor's own `ready`
       event -- the displacement happened at bind-time registration, with
       zero refresh ticks (the first periodic tick is only scheduled at
       ready + interval, `crates/atm-graft/src/runtime/lease_client.rs`);
    4. the successor delivered the row's nudge marker.

    `crash_recovery_ms`, `successor_spawn_to_ready_ms`, and
    `lease_displaced_at_ms` are diagnostics: recorded, never asserted.
    """
    pre_crash_endpoint = pre_crash_lease.get("endpoint")
    verdict: dict[str, Any] = {
        "pre_crash_endpoint": pre_crash_endpoint,
        "pre_crash_registered_at": pre_crash_lease.get("registered_at"),
        "successor_ready_at_ns": successor_ready_at_ns,
        "successor_lease_count": len(leases_after),
        "crash_recovery_ms": round((delivered_at_ns - restart_at_ns) / 1_000_000, 3),
        "successor_spawn_to_ready_ms": round((successor_ready_at_ns - restart_at_ns) / 1_000_000, 3),
        "successor_endpoint": None,
        "successor_registered_at": None,
        "lease_displaced_at_ms": None,
        "displaced_at_bind": False,
        "error": None,
    }
    failures: list[str] = []
    if len(leases_after) != 1:
        failures.append(f"expected exactly one successor lease for {RECEIVER}, found {len(leases_after)}")
    else:
        lease = leases_after[0]
        verdict["successor_endpoint"] = lease.get("endpoint")
        verdict["successor_registered_at"] = lease.get("registered_at")
        if not lease.get("endpoint") or lease.get("endpoint") == pre_crash_endpoint:
            failures.append(
                f"successor lease endpoint {lease.get('endpoint')!r} is not a fresh bind (pre-crash {pre_crash_endpoint!r})"
            )
        try:
            registered_ns = parse_rfc3339_ns(str(lease.get("registered_at")))
        except ValueError as error:
            failures.append(f"successor lease registered_at unparsable: {error}")
        else:
            verdict["lease_displaced_at_ms"] = round((registered_ns - restart_at_ns) / 1_000_000, 3)
            if registered_ns > successor_ready_at_ns:
                failures.append(
                    "successor lease was registered after the successor's ready event "
                    f"(+{(registered_ns - successor_ready_at_ns) / 1_000_000:.3f} ms): displaced by a refresh tick, not at bind"
                )
    if not delivery_matched:
        failures.append("successor did not deliver the row's nudge marker")
    if failures:
        verdict["error"] = "; ".join(failures)
    else:
        verdict["displaced_at_bind"] = True
    return verdict


def run_scenario(args: argparse.Namespace, row: str, action: str) -> dict[str, Any]:
    started_at = time.time_ns()
    # A manually-managed `mkdtemp` (not `tempfile.TemporaryDirectory`'s
    # `with`-block auto-cleanup) deliberately, mirroring
    # `run_aq4_transfer_evidence.py`'s `run_scenario`: that context
    # manager's `__exit__` calls `cleanup()`, which can raise while
    # unwinding past this row's own `return record` below, discarding the
    # already-computed record entirely. Cleanup now happens explicitly,
    # below, through `_remove_tree_tolerant`, which can never raise past
    # this function -- and only runs after the record is fully populated
    # and every child process this row owns has already been stopped.
    directory = tempfile.mkdtemp(prefix=f"aq1-9-{row}-")
    root = Path(directory)
    env, workspace = fixture_environment(root, args.atm)
    home = Path(env["HOME"])
    native_env = {key: env[key] for key in NATIVE_SESSION_ENV_KEYS}
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
    with scoped_process_environment(native_env):
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
            import atm_graft

            sender_session = atm_graft.PyGraftSession(atm_graft.PyAgentAddress(SENDER, TEAM, None))
            receiver_address = atm_graft.PyAgentAddress(RECEIVER, TEAM, None)
            if action == "daemon_restart":
                record["restart_at_ns"] = time.time_ns()
                record["daemon_stop"] = daemon.stop()
                record["daemon_after"] = daemon.start()
                refresh_sender_after_daemon_restart(sender_session, record)
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
                verdict = classify_crash_recovery(
                    pre_crash_lease=receiver_lease(record["doctor_before"]),
                    leases_after=receiver_leases(record["doctor_after"]),
                    successor_ready_at_ns=successor_ready_at_ns(record),
                    restart_at_ns=int(record["restart_at_ns"]),
                    delivered_at_ns=delivered_at,
                    delivery_matched=marker in nudge.get("body", ""),
                )
                record.update(verdict)
                if not verdict["displaced_at_bind"]:
                    record["status"] = "fail"
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
    # Scratch-directory teardown happens last, strictly after the row's
    # record is fully populated above -- a teardown failure can only ever
    # attach a `cleanup_warning` to an already-complete record, never
    # discard it.
    cleanup_warning = _remove_tree_tolerant(root)
    if cleanup_warning is not None:
        record["cleanup_warning"] = cleanup_warning
    return record


def _evidence_output_paths(args: argparse.Namespace) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ1.9"
    return evidence_dir / f"restart-matrix-{args.host}.json", evidence_dir / f"restart-matrix-{args.host}.md"


def _clear_stale_evidence(*paths: Path) -> None:
    """Deletes any pre-existing evidence file at these exact output paths
    before the matrix runs. Evidence directories are committed to the
    repo, so without this a harness that crashes before `write_evidence`
    ever runs (see `main`'s top-level guard around the three
    `run_scenario` rows) would otherwise leave the previous, stale run's
    committed file in place -- and a CI workflow's `if: always()`
    artifact-upload step would then publish that stale file as if it were
    fresh for this run. Deleting first means a genuine crash the guard
    itself cannot recover from (for example the interpreter being killed
    outright) leaves this run's evidence *missing*, never *stale* -- an
    honest signal `if-no-files-found: warn` already tolerates. Mirrors
    `run_aq4_transfer_evidence.py`'s `_clear_stale_evidence`.
    """
    for path in paths:
        path.unlink(missing_ok=True)


def write_evidence(args: argparse.Namespace, records: list[dict[str, Any]]) -> tuple[Path, Path]:
    json_path, markdown_path = _evidence_output_paths(args)
    json_path.parent.mkdir(parents=True, exist_ok=True)
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
    crashed = [record for record in records if record.get("status") == "harness_crashed"]
    if crashed:
        lines += [
            "",
            "The harness raised an unhandled exception before it could "
            "finish running every row. This transcript is written by a "
            "top-level guard in `main` specifically so a crash can never "
            "leave this run's evidence stale (a previous run's committed "
            "file, reused unchanged) or missing (no file at all) -- see "
            "`main`'s top-level `try`/`except` around the three "
            "`run_scenario` rows.",
        ]
        for record in crashed:
            lines += [
                "",
                f"Error: `{record.get('error')}`",
                "",
                "```",
                record.get("traceback", "").rstrip(),
                "```",
            ]
    lines.extend(
        [
            "",
            "The daemon restart row keeps the receiver worker alive. The receiver restart row performs a clean close and immediate replacement. The crash row uses SIGKILL and starts the successor inside the active lease window; it passes only when `atm doctor --json` shows exactly one lease for the receiver at a new endpoint whose `registered_at` is at or before the successor's own `ready` event (`displaced_at_bind`: displaced by bind-time registration, zero refresh ticks) and the successor delivers the marker. `crash_recovery_ms`, `successor_spawn_to_ready_ms`, and `lease_displaced_at_ms` are recorded as diagnostics only, never asserted.",
            "",
            "A `returncode` of 1 in `receiver_stop` / `daemon_cleanup` on Windows is `TerminateProcess` semantics for a terminated or killed child (harmless; recorded for provenance only).",
            "",
            "The m5 live run must be executed on m5; no remote result is inferred from this local artifact.",
        ]
    )
    cleanup_warnings = [record["cleanup_warning"] for record in records if record.get("cleanup_warning")]
    for warning in cleanup_warnings:
        lines += [
            "",
            "**Cleanup warning** (best-effort only; does not affect the "
            f"row status above): `{warning}`",
        ]
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
    # Deleted before any other work (and only for the top-level matrix
    # invocation -- never a `--worker` subprocess, which never writes
    # evidence and must not race the parent's own evidence files): a
    # harness crash the top-level guard below cannot itself recover from
    # must leave this run's evidence missing, never a stale copy of a
    # previous run's committed file.
    _clear_stale_evidence(*_evidence_output_paths(args))
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    require_clean_host()
    try:
        records = [
            run_scenario(args, "daemon-restart-live-receiver", "daemon_restart"),
            run_scenario(args, "receiver-restart-live-daemon", "receiver_restart"),
            run_scenario(args, "receiver-crash-within-window", "receiver_crash_within_window"),
        ]
    except Exception as error:  # noqa: BLE001 - a crash must still produce evidence, not none
        # Per-row structure: a single crashed row stands in for the whole
        # matrix here rather than three "fail" rows, since a harness crash
        # (as opposed to an in-row `run_scenario` failure, already caught
        # and recorded by that function's own try/except) means no row
        # ever finished running.
        records = [
            {
                "id": "harness_crashed",
                "action": "harness_crashed",
                "host": args.host,
                "status": "harness_crashed",
                "error": f"{type(error).__name__}: {error}",
                "traceback": traceback.format_exc(),
            }
        ]
    json_path, markdown_path = write_evidence(args, records)
    passed = all(record.get("status") == "pass" for record in records)
    print(f"{'PASS' if passed else 'FAIL'} restart matrix evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())

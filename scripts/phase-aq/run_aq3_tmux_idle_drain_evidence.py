#!/usr/bin/env python3
"""Run AQ3's live tmux idle-transition-drain evidence scenario.

This drives the real `atm-daemon` idle-transition drain path
(`crates/atm-daemon-bootstrap/src/queue_drain.rs::DrainOnTransitionSink`)
end to end: a real, owned `atm-daemon` process, a real `atm` CLI client, and a
real tmux server, communicating over the daemon's production loopback/IPC
endpoint. It proves:

1. A queue-kind message (`atm queue`, `NudgeMode::Deferred`) sent to a member
   registered with a tmux receiver pane (`LocalMessageReceivedBackend::Tmux`)
   is held pending until that member's `ActiveToolUse -> Idle` heartbeat
   transition lands, at which point exactly the oldest pending message is
   drained into the tmux pane (AC1, AC7).
2. A second queue-kind message sent before the first drain remains pending
   until the *next* Idle transition, proving FIFO, one-message-per-transition
   drain semantics (AC1).
3. A steer-kind message (`atm send`, `NudgeMode::Immediate`) to the same
   member is delivered to the pane immediately, regardless of the member's
   activity state -- it never touches the pending-queue store this scenario
   exercises for queue-kind messages (`AQ1 D4`: nothing claimable through
   `PendingNudgeStore` is ever Steer-kind).

Mechanism notes:

- The daemon's tmux received-hook emitter
  (`received_hook_selector.rs::TokioTmuxReceivedHook`) always shells out to a
  bare `tmux <args>` with no `-L`/`-S` flag, inheriting whatever tmux
  resolves by default in its own process environment. This runner starts an
  isolated, scratch tmux server (`tmux -L aq3-<rand>`) so it never touches an
  ambient tmux server that might be running on a shared host, then launches
  the daemon with a `TMUX=<socket_path>,0,0` environment variable -- the same
  mechanism tmux itself uses to resolve an ambient server when a shell is
  already running inside a tmux session -- so the daemon's unqualified `tmux`
  invocations transparently reach this scratch server. `--daemon` and `TMUX`
  scoping only affect the daemon subprocess; this runner's own `tmux -L
  aq3-<rand> ...` calls always name the scratch socket explicitly.
- Both message kinds are read from `atm_core::send::NudgeMode`, not from the
  member's live activity state: `atm queue` always persists a durable
  `Deferred` queue marker (AQ1's `PendingNudgeStore`), and `atm send` always
  dispatches `Immediate`/`Steer`. This scenario therefore does not need to
  simulate a "busy" member before sending queue-kind messages; the deferred
  marker is set unconditionally by `atm queue`, and only the drain (idle
  transition here) determines *when* it is delivered.
- Single-drain-per-transition is asserted against the daemon's cumulative
  `queue_messages_drained_total` health counter (`atm doctor --json`), not
  pane text occurrence counts: tmux echoes typed input in addition to
  whatever the pane's foreground command echoes back, so raw substring counts
  in `tmux capture-pane` output are not a reliable one-shot signal. Pane
  capture is used here only to prove presence, ordering (FIFO), and negative
  absence (the second message is not yet delivered). The counter read is
  polled (`wait_for_drained_counter_at_least`), not read once immediately
  after `wait_for_pane` returns: `TokioTmuxReceivedHook::emit_received_message`
  makes the rendered nudge visible in the pane on its *first* `send-keys`
  call, but only clears the pending marker and increments this counter after
  two more sequential tmux round trips separated by the deliberate 275ms
  `TMUX_DOUBLE_ENTER_DELAY` -- an immediate single read races that tail
  latency and reproducibly under-reads the counter.
- This harness exercises deliverable 2 (the idle-transition drain), not
  deliverable 3 (the 30-second periodic recovery sweep): deliverable 3's
  restart-recovery behavior is covered by `queue_drain.rs`'s own unit and
  integration tests and does not need a live 30+ second wait in CI to be
  meaningfully proven here.
- The daemon this repository ships is a genuine host-account singleton
  (`atm_core::home::current_host_runtime_scope` intentionally ignores
  `ATM_HOME`, `HOME`, and the current directory). Like
  `scripts/phase-aq/run_hermes_atm_restart_matrix.py` (AQ1.9) and
  `scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py` (AQ2.5),
  this runner refuses to start when an ambient `atm-daemon` already owns that
  lock, and it refuses to run (recording a `skipped_no_tmux` status) on a
  host with no `tmux` binary, such as a Windows runner.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any
import uuid


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq3-tmux-idle-drain"
SENDER = "aq3-idle-sender"
RECEIVER = "aq3-idle-receiver"
READY_TIMEOUT_SECONDS = 15.0
STEER_BODY = "aq3-steer-immediate"
QUEUE_BODY_ONE = "aq3-queue-item-one"
QUEUE_BODY_TWO = "aq3-queue-item-two"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="local", help="evidence host label, for example local or a dedicated account name")
    parser.add_argument(
        "--evidence-dir",
        type=Path,
        default=None,
        help="directory for the JSON and Markdown evidence files",
    )
    parser.add_argument(
        "--daemon",
        type=Path,
        default=Path(os.environ.get("ATM_DAEMON_BIN", ROOT / "target" / "debug" / "atm-daemon")),
        help="owned atm-daemon binary",
    )
    parser.add_argument(
        "--atm",
        type=Path,
        default=Path(os.environ.get("ATM_BIN", ROOT / "target" / "debug" / "atm")),
        help="matched atm CLI binary",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=READY_TIMEOUT_SECONDS,
        help="daemon readiness / CLI / tmux round-trip timeout in seconds",
    )
    return parser.parse_args()


def tmux_available() -> bool:
    return shutil.which("tmux") is not None


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


def fixture_environment(root: Path) -> dict[str, str]:
    home = root / "home"
    atm_home = root / "atm-home"
    logs = root / "logs"
    hook_state = root / "hook-state"
    temp_dir = root / "tmp"
    for directory in (home, atm_home, logs, hook_state, temp_dir):
        directory.mkdir(parents=True)
    return {
        **os.environ,
        "HOME": str(home),
        "ATM_HOME": str(atm_home),
        "ATM_CONFIG_HOME": str(atm_home),
        "ATM_TEAM": TEAM,
        "ATM_LOG": "debug",
        "ATM_LOG_DIR": str(logs),
        "ATM_HOOK_STATE_DIR": str(hook_state),
        "TMPDIR": str(temp_dir),
        "TMP": str(temp_dir),
        "TEMP": str(temp_dir),
    }


def run_cli(atm: Path, env: dict[str, str], args: list[str], *, identity: str, timeout: float) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(atm), *args],
        cwd=ROOT,
        env={**env, "ATM_IDENTITY": identity},
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def add_roster_member(
    atm: Path,
    env: dict[str, str],
    home: Path,
    member: str,
    timeout: float,
    *,
    backend: str | None = None,
    target: str | None = None,
) -> dict[str, Any]:
    args = ["teams", "add-member", TEAM, member, "--home-dir", str(home / member), "--json"]
    if backend is not None:
        args += ["--backend", backend]
    if target is not None:
        args += ["--target", target]
    completed = run_cli(atm, env, args, identity=SENDER, timeout=timeout)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"could not add {member} to the isolated roster: {detail}")
    return {"argv": completed.args, "stdout": completed.stdout.strip()}


def start_tmux_server(socket_name: str, session: str, timeout: float) -> tuple[str, str]:
    """Start an isolated, scratch tmux server and return its pane id and socket path."""
    completed = subprocess.run(
        ["tmux", "-L", socket_name, "new-session", "-d", "-P", "-F", "#{pane_id}", "-s", session, "cat"],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"could not start scratch tmux server: {completed.stderr.strip()}")
    pane = completed.stdout.strip()
    if not pane:
        raise RuntimeError("scratch tmux server did not return a pane id")
    socket_path_result = subprocess.run(
        ["tmux", "-L", socket_name, "display-message", "-p", "-t", session, "#{socket_path}"],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    if socket_path_result.returncode != 0:
        raise RuntimeError(f"could not resolve scratch tmux socket path: {socket_path_result.stderr.strip()}")
    socket_path = socket_path_result.stdout.strip()
    if not socket_path:
        raise RuntimeError("scratch tmux server did not return a socket path")
    return pane, socket_path


def stop_tmux_server(socket_name: str) -> None:
    subprocess.run(["tmux", "-L", socket_name, "kill-server"], capture_output=True, check=False)


def capture_pane(socket_name: str, pane: str) -> str:
    completed = subprocess.run(
        ["tmux", "-L", socket_name, "capture-pane", "-p", "-t", pane, "-S", "-200"],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"could not capture scratch tmux pane: {completed.stderr.strip()}")
    return completed.stdout


def wait_for_pane(socket_name: str, pane: str, marker: str, timeout: float) -> str:
    deadline = time.monotonic() + timeout
    latest = ""
    while time.monotonic() < deadline:
        latest = capture_pane(socket_name, pane)
        if marker in latest:
            return latest
        time.sleep(0.05)
    raise TimeoutError(f"tmux pane did not receive {marker!r} within {timeout}s; latest={latest!r}")


def _drain_ready_lines(process: subprocess.Popen[str], timeout: float) -> tuple[list[str], bool]:
    """Read `process.stdout` until `ATM_DAEMON_READY`, honoring `timeout` even
    when the child produces no output at all (FTQ-001).

    A plain blocking `readline()` call has no notion of an overall deadline:
    if the child never writes a line (for example it hangs before its first
    flush, or crashes without closing the pipe promptly), the caller blocks
    past `timeout` waiting on that single call. The read runs on a daemon
    background thread instead and this loop drains it through a queue with a
    per-call `timeout=remaining`, so the deadline is enforced even against
    the very first read.
    """
    line_queue: "queue.Queue[str | None]" = queue.Queue()

    def _pump_stdout() -> None:
        stream = process.stdout
        if stream is None:
            line_queue.put(None)
            return
        try:
            for line in iter(stream.readline, ""):
                line_queue.put(line)
        finally:
            line_queue.put(None)

    reader = threading.Thread(target=_pump_stdout, daemon=True)
    reader.start()

    deadline = time.monotonic() + timeout
    lines: list[str] = []
    ready = False
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        try:
            line = line_queue.get(timeout=remaining)
        except queue.Empty:
            break
        if line is None:
            break
        lines.append(line.rstrip("\n"))
        if line.strip() == "ATM_DAEMON_READY":
            ready = True
            break
    return lines, ready


def start_daemon(daemon: Path, env: dict[str, str], timeout: float) -> dict[str, Any]:
    process = subprocess.Popen(
        [str(daemon), "--peer-wire-security", "plaintext-test"],
        cwd=ROOT,
        env={**env, "ATM_DAEMON_READY_STDOUT": "1"},
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    lines, ready = _drain_ready_lines(process, timeout)
    stderr_tail = ""
    if process.poll() is not None:
        stderr_tail = (process.stderr.read() if process.stderr is not None else "").strip()
    return {
        "process": process,
        "ready": ready,
        "pid": process.pid,
        "stdout_tail": lines,
        "exited_early": process.poll() is not None,
        "returncode": process.returncode,
        "stderr_tail": stderr_tail,
    }


def stop_daemon(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def heartbeat(atm: Path, env: dict[str, str], member: str, activity: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(
        atm,
        env,
        ["_internal-heartbeat", "--activity", activity, "--as", member, "--team", TEAM],
        identity=member,
        timeout=timeout,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"heartbeat --activity {activity} for {member} failed: {detail}")
    return {"activity": activity, "member": member, "returncode": completed.returncode}


def send_message(atm: Path, env: dict[str, str], timeout: float, *, verb: str, body: str) -> dict[str, Any]:
    completed = run_cli(
        atm,
        env,
        [verb, f"{RECEIVER}@{TEAM}", body],
        identity=SENDER,
        timeout=timeout,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"`atm {verb}` failed: {detail}")
    return {"verb": verb, "body": body, "stdout": completed.stdout.strip()}


def diagnostic_command(
    atm: Path,
    env: dict[str, str],
    args: list[str],
    *,
    identity: str,
    timeout: float,
) -> dict[str, Any]:
    completed = run_cli(atm, env, args, identity=identity, timeout=timeout)
    return {
        "argv": completed.args,
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def pending_diagnostics(atm: Path, env: dict[str, str], timeout: float) -> dict[str, Any]:
    return {
        "peek": diagnostic_command(
            atm, env, ["peek", "--json", "--all", "--team", TEAM], identity=RECEIVER, timeout=timeout
        ),
        "doctor": diagnostic_command(
            atm, env, ["doctor", "--json", "--team", TEAM], identity=RECEIVER, timeout=timeout
        ),
    }


def drained_counter(atm: Path, env: dict[str, str], timeout: float) -> int:
    """Cumulative `queue_messages_drained_total` from the daemon's own health report."""
    completed = run_cli(atm, env, ["doctor", "--json", "--team", TEAM], identity=RECEIVER, timeout=timeout)
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"`atm doctor --json` returned unparsable output: {completed.stdout!r}") from error
    runtime_status = payload.get("runtime_status") or {}
    return int(runtime_status.get("queue_messages_drained_total", 0))


def wait_for_drained_counter_at_least(atm: Path, env: dict[str, str], timeout: float, minimum: int) -> int:
    """Poll `queue_messages_drained_total` until it reaches `minimum` or `timeout` elapses.

    `wait_for_pane` only proves the rendered nudge text became visible, which
    happens on the *first* of three sequential tmux `send-keys` calls inside
    `TokioTmuxReceivedHook::emit_received_message`. The daemon does not clear
    the pending marker or increment this counter until *after* the second and
    third `send-keys` calls -- separated by the deliberate
    `TMUX_DOUBLE_ENTER_DELAY` (275ms) -- complete. A single immediate read
    right after `wait_for_pane` returns races that tail latency and
    reproducibly under-reads the counter; polling here is required, not
    optional.
    """
    deadline = time.monotonic() + timeout
    latest = drained_counter(atm, env, timeout)
    while latest < minimum and time.monotonic() < deadline:
        time.sleep(0.05)
        latest = drained_counter(atm, env, timeout)
    return latest


def daemon_log_tail(env: dict[str, str]) -> dict[str, Any]:
    candidates = [
        Path(env["ATM_HOME"]) / "logs" / "atm.log.jsonl",
        Path(env.get("ATM_LOG_DIR", "")) / "atm.log.jsonl",
        Path(env["ATM_HOME"]) / "atm.log.jsonl",
    ]
    for path in candidates:
        if not path.is_file():
            continue
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        filtered = [
            line
            for line in lines
            if any(term in line.lower() for term in ("queue", "pending", "drain", "idle_transition"))
        ]
        return {"path": str(path), "lines": filtered[-60:]}
    return {"path": str(candidates[0]), "lines": [], "missing": True}


def run_scenario(args: argparse.Namespace) -> dict[str, Any]:
    started_at = time.time_ns()
    record: dict[str, Any] = {
        "sprint": "AQ3",
        "host": args.host,
        "started_at_ns": started_at,
        "status": "blocked",
    }

    if not tmux_available():
        record["status"] = "skipped_no_tmux"
        record["note"] = (
            "tmux is not available on this runner (for example a Windows "
            "host); the tmux idle-drain scenario cannot execute without it. "
            "This is a fail-closed skip, not a failure."
        )
        return record

    ambient = ambient_daemon_pids()
    if ambient:
        record["status"] = "blocked_ambient_daemon"
        record["ambient_daemon_pids"] = ambient
        record["error"] = (
            "an ATM daemon already owns this OS account's singleton runtime "
            "root (current_host_runtime_scope ignores ATM_HOME/HOME by "
            "design); a second daemon cannot be safely started on this host "
            "without risking the ambient session. Run on a dedicated host / "
            "OS account with no ambient atm-daemon to produce positive-path "
            "evidence."
        )
        return record

    socket_name = f"aq3-{uuid.uuid4().hex[:10]}"
    session = f"aq3-idle-drain-{os.getpid()}"
    tmux_started = False
    daemon_process: subprocess.Popen[str] | None = None
    try:
        with tempfile.TemporaryDirectory(prefix="aq3-tmux-idle-drain-") as directory:
            root = Path(directory)
            env = fixture_environment(root)
            home = Path(env["HOME"])

            pane, socket_path = start_tmux_server(socket_name, session, args.timeout)
            tmux_started = True
            record["tmux"] = {"socket": socket_name, "session": session, "pane": pane, "socket_path": socket_path}

            record["roster"] = {
                "sender": add_roster_member(args.atm, env, home, SENDER, args.timeout),
                "receiver": add_roster_member(
                    args.atm, env, home, RECEIVER, args.timeout, backend="tmux", target=pane
                ),
            }

            # Only the daemon subprocess needs bridging to the scratch tmux
            # server: its received-hook emitter shells out to a bare `tmux`
            # with no `-L`/`-S`, so it resolves the ambient server the same
            # way tmux does for a shell already inside a session -- from
            # `$TMUX`. The CLI client this runner drives never invokes tmux
            # itself, so its own env is untouched.
            daemon_env = {**env, "TMUX": f"{socket_path},0,0"}
            daemon = start_daemon(args.daemon, daemon_env, args.timeout)
            daemon_process = daemon.pop("process")
            record["daemon_start"] = daemon
            if not daemon["ready"]:
                record["status"] = "blocked_daemon_start_failed"
                record["error"] = daemon.get("stderr_tail") or "daemon did not report ready"
                return record

            record["initial_heartbeat"] = heartbeat(args.atm, env, RECEIVER, "active-tool-use", args.timeout)
            counters_before = drained_counter(args.atm, env, args.timeout)
            record["counters_before"] = counters_before

            # --- Steer-kind: delivered immediately, never queued. ---
            steer_send = send_message(args.atm, env, args.timeout, verb="send", body=STEER_BODY)
            pane_after_steer = wait_for_pane(socket_name, pane, STEER_BODY, args.timeout)
            record["steer_kind_immediate"] = {
                "send": steer_send,
                "pane_after_send": pane_after_steer,
                "delivered_before_any_idle_transition": STEER_BODY in pane_after_steer,
            }

            # --- Queue-kind: both persisted as Deferred pending rows up front. ---
            queue_sends = [
                send_message(args.atm, env, args.timeout, verb="queue", body=QUEUE_BODY_ONE),
                send_message(args.atm, env, args.timeout, verb="queue", body=QUEUE_BODY_TWO),
            ]
            record["queue_kind_sends"] = queue_sends
            record["diagnostics_before_drain"] = pending_diagnostics(args.atm, env, args.timeout)

            # --- First Idle transition drains exactly the oldest pending row. ---
            first_transition = heartbeat(args.atm, env, RECEIVER, "idle", args.timeout)
            pane_after_first = wait_for_pane(socket_name, pane, QUEUE_BODY_ONE, args.timeout)
            counters_after_first = wait_for_drained_counter_at_least(
                args.atm, env, args.timeout, counters_before + 1
            )
            record["idle_transition_drain_one"] = {
                "heartbeat": first_transition,
                "pane_after": pane_after_first,
                "second_item_not_yet_present": QUEUE_BODY_TWO not in pane_after_first,
                "counters_after": counters_after_first,
                "drained_delta": counters_after_first - counters_before,
                "diagnostics": pending_diagnostics(args.atm, env, args.timeout),
            }

            # --- A second genuine Active->Idle transition drains the sibling. ---
            heartbeat(args.atm, env, RECEIVER, "active-tool-use", args.timeout)
            second_transition = heartbeat(args.atm, env, RECEIVER, "idle", args.timeout)
            pane_after_second = wait_for_pane(socket_name, pane, QUEUE_BODY_TWO, args.timeout)
            counters_after_second = wait_for_drained_counter_at_least(
                args.atm, env, args.timeout, counters_after_first + 1
            )
            record["idle_transition_drain_two"] = {
                "heartbeat": second_transition,
                "pane_after": pane_after_second,
                "counters_after": counters_after_second,
                "drained_delta": counters_after_second - counters_after_first,
                "diagnostics": pending_diagnostics(args.atm, env, args.timeout),
            }

            record["daemon_log_tail"] = daemon_log_tail(env)

            fifo_order_confirmed = pane_after_second.index(QUEUE_BODY_ONE) < pane_after_second.index(QUEUE_BODY_TWO)
            single_drain_confirmed = (
                record["idle_transition_drain_one"]["drained_delta"] == 1
                and record["idle_transition_drain_two"]["drained_delta"] == 1
            )
            record["fifo_order_confirmed"] = fifo_order_confirmed
            record["single_drain_per_transition_confirmed"] = single_drain_confirmed
            record["status"] = (
                "pass"
                if (
                    record["steer_kind_immediate"]["delivered_before_any_idle_transition"]
                    and record["idle_transition_drain_one"]["second_item_not_yet_present"]
                    and fifo_order_confirmed
                    and single_drain_confirmed
                )
                else "fail"
            )
    except Exception as error:  # noqa: BLE001 - evidence must retain the failure
        record["status"] = "fail"
        record["error"] = f"{type(error).__name__}: {error}"
    finally:
        stop_daemon(daemon_process)
        if tmux_started:
            stop_tmux_server(socket_name)
        record["finished_at_ns"] = time.time_ns()
    return record


def write_evidence(args: argparse.Namespace, record: dict[str, Any]) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ3"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    json_path = evidence_dir / f"tmux-idle-drain-{args.host}.json"
    markdown_path = evidence_dir / f"tmux-idle-drain-{args.host}.md"
    payload = {
        "schema_version": 1,
        "sprint": "AQ3",
        "host": args.host,
        "commit": subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip(),
        "record": record,
    }
    json_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    lines = [
        "# AQ3 tmux idle-transition-drain evidence",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{payload['commit']}`",
        f"Status: **{record['status'].upper()}**",
        "",
    ]
    if record["status"] == "skipped_no_tmux":
        lines += [
            "This host has no `tmux` binary (for example a Windows runner). "
            "The live tmux idle-drain scenario needs a real tmux server and "
            "cannot execute here; this is a fail-closed skip, not a failure.",
        ]
    elif record["status"] == "blocked_ambient_daemon":
        lines += [
            "This host has an ambient, already-running `atm-daemon` "
            f"(pid(s) {record['ambient_daemon_pids']}) that legitimately owns "
            "the OS account's singleton runtime lock "
            "(`atm_core::home::current_host_runtime_scope` intentionally "
            "ignores `ATM_HOME`/`HOME` -- see `DaemonOwnerGuard`). This runner "
            "refuses to start a second daemon on this account rather than "
            "risk the ambient session, exactly as "
            "`run_hermes_atm_restart_matrix.py` (AQ1.9) and "
            "`run_aq25_queue_delivery_trigger_evidence.py` (AQ2.5) do for the "
            "same reason.",
            "",
            "Run this script on a dedicated OS account with no ambient "
            "`atm-daemon` to produce positive-path evidence:",
            "",
            "```bash",
            "python3 scripts/phase-aq/run_aq3_tmux_idle_drain_evidence.py \\",
            "  --host <dedicated-account-label> \\",
            "  --daemon target/release/atm-daemon \\",
            "  --atm target/release/atm \\",
            "  --evidence-dir docs/plans/phase-aq/evidence/AQ3",
            "```",
        ]
    else:
        lines += [
            "## Steer-kind message: immediate delivery",
            "",
            f"stdout: `{record.get('steer_kind_immediate', {}).get('send', {}).get('stdout')}`",
            "",
            f"Delivered before any idle transition: **{record.get('steer_kind_immediate', {}).get('delivered_before_any_idle_transition')}**",
            "",
            "## Queue-kind messages: FIFO idle-transition drain",
            "",
            "| Transition | drained_delta | pane contains second item yet? |",
            "| --- | --- | --- |",
            (
                "| 1st Active->Idle | "
                f"{record.get('idle_transition_drain_one', {}).get('drained_delta')} | "
                f"{not record.get('idle_transition_drain_one', {}).get('second_item_not_yet_present', True)} |"
            ),
            (
                "| 2nd Active->Idle | "
                f"{record.get('idle_transition_drain_two', {}).get('drained_delta')} | n/a |"
            ),
            "",
            f"FIFO order confirmed: **{record.get('fifo_order_confirmed')}**",
            "",
            f"Single drain per transition confirmed (via `queue_messages_drained_total`): **{record.get('single_drain_per_transition_confirmed')}**",
        ]
        if record["status"] != "pass":
            lines += ["", "The runner recorded a failure; inspect the JSON record for the full diagnostic trail."]
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    record = run_scenario(args)
    json_path, markdown_path = write_evidence(args, record)
    print(f"{record['status'].upper()} AQ3 tmux idle-drain evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if record["status"] in ("pass", "blocked_ambient_daemon", "skipped_no_tmux") else 1


if __name__ == "__main__":
    raise SystemExit(main())

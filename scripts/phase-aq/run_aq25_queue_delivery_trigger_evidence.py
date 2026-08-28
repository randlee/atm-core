#!/usr/bin/env python3
"""Run the AQ2.5 bare-CLI queue-delivery-trigger live evidence scenario.

This drives the committed reference hook script (``scripts/hooks/
atm_queue_hook.py``) as a real ``Stop`` lifecycle hook, invoking the real
``atm`` CLI binary against a real, owned ``atm-daemon`` process communicating
over its production loopback/IPC endpoint. It proves two things end to end:

1. Two queue-kind messages sent to a bare-CLI member (no local backend, no
   Graft lease) drain **one per Stop pull**, oldest first, each emitting the
   literal Claude ``{"decision": "block", "reason": "..."}`` JSON, and the
   following Stop proceeds with no output once the FIFO is empty.
2. Two steer-kind messages sent to the same bare-CLI member arrive **in full
   on the next single Stop pull** (steer items always drain together).

The daemon this repository ships is a genuine host-account singleton
(``atm_core::home::current_host_runtime_scope`` intentionally ignores
``ATM_HOME``, ``HOME``, and the current directory; it resolves the OS
account's real profile directory via ``getpwuid``/``SHGetKnownFolderPath``).
That is deliberate: it is the same boundary ``DaemonOwnerGuard`` enforces with
an OS-level exclusive file lock so two daemons never race the same host. This
runner therefore refuses to start when an ambient ``atm-daemon`` already owns
that lock, exactly like ``scripts/phase-aq/run_hermes_atm_restart_matrix.py``
(AQ1.9) does for the same reason — it must never contend with, and can never
safely replace, a real daemon already serving another session on the same OS
account. Run it on a dedicated OS account (or a host with no ambient ATM
daemon) to produce the committed positive-path evidence.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import traceback
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq2-5-bare-cli"
SENDER = "aq2-5-sender"
RECEIVER = "aq2-5-receiver"
READY_TIMEOUT_SECONDS = 15.0


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
        help="daemon readiness / CLI round-trip timeout in seconds",
    )
    return parser.parse_args()


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
    for directory in (home, atm_home, logs, hook_state):
        directory.mkdir(parents=True)
    temp_dir = root / "tmp"
    temp_dir.mkdir()
    return {
        **os.environ,
        "HOME": str(home),
        "ATM_HOME": str(atm_home),
        "ATM_CONFIG_HOME": str(atm_home),
        "ATM_TEAM": TEAM,
        "ATM_LOG": "debug",
        "ATM_LOG_DIR": str(logs),
        "ATM_HOOK_STATE_DIR": str(hook_state),
        "ATM_HOOK_DEBOUNCE_SECONDS": "0.02",
        "ATM_HOOK_TIMEOUT_SECONDS": "5",
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


def add_roster_member(atm: Path, env: dict[str, str], home: Path, member: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(
        atm,
        env,
        ["teams", "add-member", TEAM, member, "--home-dir", str(home), "--json"],
        identity=SENDER,
        timeout=timeout,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"could not add {member} to the isolated roster: {detail}")
    return {"argv": completed.args, "stdout": completed.stdout.strip()}


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
    deadline = time.monotonic() + timeout
    lines: list[str] = []
    ready = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            break
        line = process.stdout.readline() if process.stdout is not None else ""
        if line:
            lines.append(line.rstrip("\n"))
            if line.strip() == "ATM_DAEMON_READY":
                ready = True
                break
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


def run_hook(
    env: dict[str, str],
    atm: Path,
    harness: str,
    timeout: float,
    trace_path: Path,
) -> subprocess.CompletedProcess[str]:
    hook = ROOT / "scripts" / "hooks" / "atm_queue_hook.py"
    hook_env = {
        **env,
        "ATM_BIN": str(atm),
        "ATM_IDENTITY": RECEIVER,
        "ATM_TEAM": TEAM,
        "ATM_HOME": env["ATM_HOME"],
        "ATM_CONFIG_HOME": env["ATM_CONFIG_HOME"],
        "ATM_HOOK_TRACE_FILE": str(trace_path),
    }
    return subprocess.run(
        [sys.executable, str(hook), "--event", "stop", "--harness", harness],
        cwd=ROOT,
        env=hook_env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


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
            if any(term in line.lower() for term in ("queue", "pending", "nudge"))
        ]
        return {"path": str(path), "lines": filtered[-40:]}
    return {"path": str(candidates[0]), "lines": [], "missing": True}


def pull_step(env: dict[str, str], atm: Path, timeout: float, *, harness: str = "claude") -> dict[str, Any]:
    # The hook's trace captures the raw queue-get process without performing a
    # second destructive pull. The mailbox/doctor/log probes are read-only and
    # run immediately before the lifecycle Stop invocation.
    trace_path = Path(env["ATM_HOOK_STATE_DIR"]) / "last-queue-get.json"
    try:
        trace_path.unlink()
    except FileNotFoundError:
        pass
    inbox = diagnostic_command(
        atm,
        env,
        ["peek", "--json", "--all", "--team", TEAM, "--as", RECEIVER],
        identity=RECEIVER,
        timeout=timeout,
    )
    doctor = diagnostic_command(
        atm,
        env,
        ["doctor", "--json", "--team", TEAM],
        identity=RECEIVER,
        timeout=timeout,
    )
    completed = run_hook(env, atm, harness, timeout, trace_path)
    if trace_path.is_file():
        raw_queue_get: dict[str, Any] = json.loads(trace_path.read_text(encoding="utf-8"))
    else:
        raw_queue_get = {"missing": True, "path": str(trace_path)}
    step: dict[str, Any] = {
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr_tail": completed.stderr.strip()[-2000:],
        "diagnostics": {
            "raw_queue_get": raw_queue_get,
            "receiver_inbox_peek": inbox,
            "doctor": doctor,
            "daemon_log_tail": daemon_log_tail(env),
        },
        "hook_argv": [sys.executable, str(ROOT / "scripts" / "hooks" / "atm_queue_hook.py"), "--event", "stop", "--harness", harness],
        "hook_env_keys": sorted(
            key
            for key in (
                "ATM_BIN",
                "ATM_IDENTITY",
                "ATM_TEAM",
                "ATM_HOME",
                "ATM_CONFIG_HOME",
                "ATM_HOOK_STATE_DIR",
                "ATM_HOOK_DEBOUNCE_SECONDS",
                "ATM_HOOK_TIMEOUT_SECONDS",
                "ATM_HOOK_TRACE_FILE",
            )
            if key in env or key in {"ATM_BIN", "ATM_IDENTITY", "ATM_TEAM", "ATM_HOOK_TRACE_FILE"}
        ),
    }
    if step["stdout"]:
        step["block"] = json.loads(step["stdout"])
    return step


def run_scenario(args: argparse.Namespace) -> dict[str, Any]:
    started_at = time.time_ns()
    record: dict[str, Any] = {
        "sprint": "AQ2.5",
        "host": args.host,
        "started_at_ns": started_at,
        "status": "blocked",
    }
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

    with tempfile.TemporaryDirectory(prefix="aq2-5-evidence-") as directory:
        root = Path(directory)
        env = fixture_environment(root)
        home = Path(env["HOME"])
        daemon_process: subprocess.Popen[str] | None = None
        try:
            record["roster"] = {
                "sender": add_roster_member(args.atm, env, home, SENDER, args.timeout),
                "receiver": add_roster_member(args.atm, env, home, RECEIVER, args.timeout),
            }
            daemon = start_daemon(args.daemon, env, args.timeout)
            daemon_process = daemon.pop("process")
            record["daemon_start"] = daemon
            if not daemon["ready"]:
                record["status"] = "blocked_daemon_start_failed"
                record["error"] = daemon.get("stderr_tail") or "daemon did not report ready"
                return record

            # --- Scenario 1: two queue-kind messages drain one per Stop ---
            record["queue_kind_sends"] = [
                send_message(args.atm, env, args.timeout, verb="queue", body="queue-item-one"),
                send_message(args.atm, env, args.timeout, verb="queue", body="queue-item-two"),
            ]
            pull_one = pull_step(env, args.atm, args.timeout)
            pull_two = pull_step(env, args.atm, args.timeout)
            pull_empty = pull_step(env, args.atm, args.timeout)
            record["queue_kind_pulls"] = [pull_one, pull_two, pull_empty]
            queue_ok = (
                pull_one.get("block") == {"decision": "block", "reason": "queue-item-one"}
                and pull_two.get("block") == {"decision": "block", "reason": "queue-item-two"}
                and pull_empty.get("stdout") == ""
            )
            record["queue_kind_one_per_stop_confirmed"] = queue_ok

            # --- Scenario 2: two steer-kind messages drain together ---
            record["steer_kind_sends"] = [
                send_message(args.atm, env, args.timeout, verb="send", body="steer-item-one"),
                send_message(args.atm, env, args.timeout, verb="send", body="steer-item-two"),
            ]
            pull_steer = pull_step(env, args.atm, args.timeout)
            record["steer_kind_pull"] = pull_steer
            steer_ok = pull_steer.get("block") == {
                "decision": "block",
                "reason": "steer-item-one\nsteer-item-two",
            }
            record["steer_kind_full_drain_confirmed"] = steer_ok

            record["doctor_after"] = run_cli(
                args.atm, env, ["doctor", "--json"], identity=RECEIVER, timeout=args.timeout
            ).stdout

            record["status"] = "pass" if queue_ok and steer_ok else "fail"
        except Exception as error:  # noqa: BLE001 - evidence must retain the failure
            record["error"] = f"{type(error).__name__}: {error}"
            record["status"] = "fail"
        finally:
            stop_daemon(daemon_process)
            record["finished_at_ns"] = time.time_ns()
    return record


def _evidence_output_paths(args: argparse.Namespace) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ2.5"
    return (
        evidence_dir / f"queue-delivery-trigger-{args.host}.json",
        evidence_dir / f"queue-delivery-trigger-{args.host}.md",
    )


def _clear_stale_evidence(*paths: Path) -> None:
    """Deletes any pre-existing evidence file at these exact output paths
    before the scenario runs, so a harness crash that never reaches
    `write_evidence` leaves this run's evidence missing, never a stale
    copy of a previous run's committed file (see AQ4 evidence run 6,
    33137262962 @ c510a4745, for the concrete failure this guards
    against).
    """
    for path in paths:
        path.unlink(missing_ok=True)


def write_evidence(args: argparse.Namespace, record: dict[str, Any]) -> tuple[Path, Path]:
    json_path, markdown_path = _evidence_output_paths(args)
    json_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "sprint": "AQ2.5",
        "host": args.host,
        "commit": subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.strip(),
        "record": record,
    }
    json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# AQ2.5 bare-CLI queue delivery-trigger evidence",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{payload['commit']}`",
        f"Status: **{record['status'].upper()}**",
        "",
    ]
    if record["status"] == "blocked_ambient_daemon":
        lines += [
            "This host has an ambient, already-running `atm-daemon` "
            f"(pid(s) {record['ambient_daemon_pids']}) that legitimately owns "
            "the OS account's singleton runtime lock "
            "(`atm_core::home::current_host_runtime_scope` intentionally "
            "ignores `ATM_HOME`/`HOME` — see `DaemonOwnerGuard`). This runner "
            "refuses to start a second daemon on this account rather than "
            "risk the ambient session, exactly as "
            "`run_hermes_atm_restart_matrix.py` (AQ1.9) does for the same "
            "reason.",
            "",
            "Run this script on a dedicated OS account with no ambient "
            "`atm-daemon` to produce positive-path evidence:",
            "",
            "```bash",
            "python3 scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py \\",
            "  --host <dedicated-account-label> \\",
            "  --daemon target/release/atm-daemon \\",
            "  --atm target/release/atm \\",
            "  --evidence-dir docs/plans/phase-aq/evidence/AQ2.5",
            "```",
        ]
    elif record["status"] == "harness_crashed":
        lines += [
            "The harness raised an unhandled exception before it could "
            "finish running the scenario. This transcript is written by a "
            "top-level guard specifically so a crash can never leave this "
            "run's evidence stale (a previous run's committed file, "
            "reused unchanged) or missing (no file at all) -- see "
            "`main`'s top-level `try`/`except` around `run_scenario`.",
            "",
            f"Error: `{record.get('error')}`",
            "",
            "```",
            record.get("traceback", "").rstrip(),
            "```",
        ]
    else:
        queue_pulls = record.get("queue_kind_pulls", [])
        lines += [
            "## Scenario 1 — two queue-kind messages drain one per Stop",
            "",
            "| Pull | stdout | Parsed block |",
            "| --- | --- | --- |",
        ]
        for index, pull in enumerate(queue_pulls, start=1):
            lines.append(f"| {index} | `{pull.get('stdout') or '(empty)'}` | `{pull.get('block')}` |")
        lines += [
            "",
            f"One-per-Stop confirmed: **{record.get('queue_kind_one_per_stop_confirmed')}**",
            "",
            "## Scenario 2 — two steer-kind messages drain together on one Stop",
            "",
            f"stdout: `{record.get('steer_kind_pull', {}).get('stdout')}`",
            "",
            f"Full-batch drain confirmed: **{record.get('steer_kind_full_drain_confirmed')}**",
        ]
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    # Deleted before any other work: a harness crash the guard below
    # cannot itself recover from must leave this run's evidence missing,
    # never a stale copy of a previous run's committed file.
    _clear_stale_evidence(*_evidence_output_paths(args))
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm binary does not exist: {args.atm}")
    try:
        record = run_scenario(args)
    except Exception as error:  # noqa: BLE001 - a crash must still produce evidence, not none
        record = {
            "sprint": "AQ2.5",
            "host": args.host,
            "status": "harness_crashed",
            "error": f"{type(error).__name__}: {error}",
            "traceback": traceback.format_exc(),
        }
    json_path, markdown_path = write_evidence(args, record)
    print(f"{record['status'].upper()} AQ2.5 queue delivery-trigger evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if record["status"] in ("pass", "blocked_ambient_daemon") else 1


if __name__ == "__main__":
    raise SystemExit(main())

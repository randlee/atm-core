#!/usr/bin/env python3
"""Run AQ3's idle-drain and recovery-sweep evidence scenarios.

The runner owns the daemon, temporary fixture roots, and tmux session that it
creates.  ATM's daemon/database scope is intentionally OS-account scoped, so
the runner refuses to start when another ``atm-daemon`` already owns the
account.  That fail-closed behavior protects an ambient session and leaves an
honest blocked JSON/Markdown transcript locally; run this script on a
dedicated account to produce positive-path evidence.

The positive path exercises three rows:

1. Three queue messages to a tmux-classified member, with each AQ2.5 Stop
   hook's idle heartbeat causing exactly one tmux nudge.
2. A pending tmux message that survives an owned daemon restart and is
   delivered by the 30-second AQ3 recovery sweep without another heartbeat.
3. Herdr and bare-CLI members receiving idle heartbeats without an AQ3 claim:
   the Herdr queue-get remains empty while the bare-CLI FIFO item is returned
   by its own queue-get path.
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
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
TEAM = "aq3-idle-drain"
SENDER = "aq3-sender"
TMUX_MEMBER = "aq3-tmux-receiver"
HERDR_MEMBER = "aq3-herdr-receiver"
BARE_MEMBER = "aq3-bare-receiver"
READY_TIMEOUT_SECONDS = 15.0
RECOVERY_SWEEP_INTERVAL_SECONDS = 30.0


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
        help="daemon and individual delivery timeout in seconds",
    )
    parser.add_argument(
        "--recovery-timeout",
        type=float,
        default=RECOVERY_SWEEP_INTERVAL_SECONDS + READY_TIMEOUT_SECONDS,
        help="maximum wait for the post-restart recovery sweep",
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
        "ATM_HOOK_DEBOUNCE_SECONDS": "0",
        "ATM_HOOK_TIMEOUT_SECONDS": "5",
        "TMPDIR": str(temp_dir),
        "TMP": str(temp_dir),
        "TEMP": str(temp_dir),
    }


def run_cli(
    atm: Path,
    env: dict[str, str],
    args: list[str],
    *,
    identity: str,
    timeout: float,
) -> subprocess.CompletedProcess[str]:
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


def add_member(
    atm: Path,
    env: dict[str, str],
    home: Path,
    member: str,
    timeout: float,
    *,
    backend: str | None = None,
    target: str | None = None,
    session: str | None = None,
) -> dict[str, Any]:
    args = ["teams", "add-member", TEAM, member, "--home-dir", str(home / member), "--json"]
    if backend is not None:
        args += ["--backend", backend]
    if target is not None:
        args += ["--target", target]
    if session is not None:
        args += ["--session", session]
    completed = run_cli(atm, env, args, identity=SENDER, timeout=timeout)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"could not add {member}: {detail}")
    return {"argv": completed.args, "stdout": completed.stdout.strip()}


def start_daemon(daemon: Path, env: dict[str, str], timeout: float) -> tuple[subprocess.Popen[str], dict[str, Any]]:
    process = subprocess.Popen(
        [str(daemon)],
        cwd=ROOT,
        env={**env, "ATM_DAEMON_READY_STDOUT": "1"},
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    deadline = time.monotonic() + timeout
    stdout_tail: list[str] = []
    ready = False
    while time.monotonic() < deadline and process.poll() is None:
        line = process.stdout.readline() if process.stdout is not None else ""
        if line:
            stdout_tail.append(line.rstrip("\n"))
            if line.strip() == "ATM_DAEMON_READY":
                ready = True
                break
    stderr_tail = ""
    if process.poll() is not None and process.stderr is not None:
        stderr_tail = process.stderr.read().strip()
    return process, {
        "pid": process.pid,
        "ready": ready,
        "stdout_tail": stdout_tail,
        "stderr_tail": stderr_tail,
        "returncode": process.returncode,
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


def start_tmux_session() -> tuple[str, str]:
    session = f"aq3-evidence-{os.getpid()}"
    completed = subprocess.run(
        ["tmux", "new-session", "-d", "-P", "-F", "#{pane_id}", "-s", session, "cat"],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"could not start owned tmux session: {completed.stderr.strip()}")
    pane = completed.stdout.strip()
    if not pane:
        raise RuntimeError("owned tmux session did not return a pane id")
    return session, pane


def stop_tmux_session(session: str | None) -> None:
    if session is None:
        return
    subprocess.run(["tmux", "kill-session", "-t", session], capture_output=True, check=False)


def capture_pane(pane: str) -> str:
    completed = subprocess.run(
        ["tmux", "capture-pane", "-p", "-t", pane, "-S", "-200"],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"could not capture owned tmux pane: {completed.stderr.strip()}")
    return completed.stdout


def wait_for_pane(pane: str, marker: str, expected_count: int, timeout: float) -> str:
    deadline = time.monotonic() + timeout
    latest = ""
    while time.monotonic() < deadline:
        latest = capture_pane(pane)
        if latest.count(marker) >= expected_count:
            return latest
        time.sleep(0.05)
    raise TimeoutError(f"tmux pane did not receive {marker!r} {expected_count} time(s); latest={latest!r}")


def run_hook(env: dict[str, str], atm: Path, timeout: float, *, identity: str = TMUX_MEMBER) -> dict[str, Any]:
    hook = ROOT / "scripts" / "hooks" / "atm_queue_hook.py"
    completed = subprocess.run(
        [sys.executable, str(hook), "--event", "stop", "--harness", "claude"],
        cwd=ROOT,
        env={**env, "ATM_BIN": str(atm), "ATM_IDENTITY": identity},
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )
    return {
        "returncode": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr_tail": completed.stderr.strip()[-2000:],
    }


def heartbeat(atm: Path, env: dict[str, str], member: str, activity: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(
        atm,
        env,
        ["_internal-heartbeat", "--activity", activity, "--as", member, "--team", TEAM],
        identity=member,
        timeout=timeout,
    )
    return {"activity": activity, "member": member, "returncode": completed.returncode, "stderr": completed.stderr.strip()}


def send_queue(atm: Path, env: dict[str, str], member: str, body: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(atm, env, ["queue", f"{member}@{TEAM}", body], identity=SENDER, timeout=timeout)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"queue send to {member} failed: {detail}")
    return {"member": member, "body": body, "stdout": completed.stdout.strip()}


def queue_get(atm: Path, env: dict[str, str], member: str, timeout: float) -> dict[str, Any]:
    completed = run_cli(
        atm,
        {**env, "ATM_IDENTITY": member},
        ["_internal-queue-get", "--as", member, "--team", TEAM],
        identity=member,
        timeout=timeout,
    )
    return {"member": member, "returncode": completed.returncode, "stdout": completed.stdout.strip(), "stderr": completed.stderr.strip()}


def run_scenario(args: argparse.Namespace) -> dict[str, Any]:
    record: dict[str, Any] = {
        "sprint": "AQ3",
        "host": args.host,
        "started_at_ns": time.time_ns(),
        "status": "blocked",
        "recovery_sweep_interval_seconds": RECOVERY_SWEEP_INTERVAL_SECONDS,
    }
    ambient = ambient_daemon_pids()
    if ambient:
        record["status"] = "blocked_ambient_daemon"
        record["ambient_daemon_pids"] = ambient
        record["error"] = (
            "an ATM daemon already owns this OS account's singleton runtime root; "
            "the runner refuses to contend with the ambient session. Run on a "
            "dedicated OS account with no ambient atm-daemon for positive evidence."
        )
        return record

    tmux_session: str | None = None
    daemon_process: subprocess.Popen[str] | None = None
    try:
        with tempfile.TemporaryDirectory(prefix="aq3-evidence-") as directory:
            root = Path(directory)
            env = fixture_environment(root)
            home = Path(env["HOME"])
            tmux_session, pane = start_tmux_session()
            record["tmux"] = {"session": tmux_session, "pane": pane}
            record["roster"] = {
                "sender": add_member(args.atm, env, home, SENDER, args.timeout),
                "tmux": add_member(args.atm, env, home, TMUX_MEMBER, args.timeout, backend="tmux", target=pane),
                "herdr": add_member(args.atm, env, home, HERDR_MEMBER, args.timeout, backend="herdr", session="aq3-session"),
                "bare_cli": add_member(args.atm, env, home, BARE_MEMBER, args.timeout),
            }
            daemon_process, daemon_start = start_daemon(args.daemon, env, args.timeout)
            record["daemon_start"] = daemon_start
            if not daemon_start["ready"]:
                record["status"] = "blocked_daemon_start_failed"
                record["error"] = daemon_start.get("stderr_tail") or "daemon did not report ready"
                return record

            heartbeat(args.atm, env, TMUX_MEMBER, "active-tool-use", args.timeout)
            idle_sends = [
                send_queue(args.atm, env, TMUX_MEMBER, "aq3-idle-one", args.timeout),
                send_queue(args.atm, env, TMUX_MEMBER, "aq3-idle-two", args.timeout),
                send_queue(args.atm, env, TMUX_MEMBER, "aq3-idle-three", args.timeout),
            ]
            idle_rows: list[dict[str, Any]] = []
            for index, sent in enumerate(idle_sends, start=1):
                hook = run_hook(env, args.atm, args.timeout)
                pane_text = wait_for_pane(pane, sent["body"], index, args.timeout)
                idle_rows.append(
                    {
                        "idle_number": index,
                        "send": sent,
                        "hook": hook,
                        "pane_occurrences": pane_text.count(sent["body"]),
                        "exactly_one_nudge_for_idle": pane_text.count(sent["body"]) == 1,
                    }
                )
            record["scenario_1_idle_transition_drain"] = {
                "rows": idle_rows,
                "exactly_one_nudge_per_idle": all(row["exactly_one_nudge_for_idle"] for row in idle_rows),
            }

            heartbeat(args.atm, env, TMUX_MEMBER, "active-tool-use", args.timeout)
            recovery_send = send_queue(args.atm, env, TMUX_MEMBER, "aq3-recovery-after-restart", args.timeout)
            pre_restart = capture_pane(pane)
            stop_daemon(daemon_process)
            daemon_process = None
            daemon_process, restart = start_daemon(args.daemon, env, args.timeout)
            recovery_pane = wait_for_pane(pane, recovery_send["body"], 1, args.recovery_timeout)
            record["scenario_2_recovery_sweep_after_restart"] = {
                "send_before_restart": recovery_send,
                "pane_before_restart": pre_restart,
                "daemon_restart": restart,
                "pane_after_restart": recovery_pane,
                "dispatched_without_idle_heartbeat": recovery_send["body"] not in pre_restart,
                "recovery_dispatch_confirmed": recovery_pane.count(recovery_send["body"]) == 1,
            }

            heartbeat(args.atm, env, HERDR_MEMBER, "active-tool-use", args.timeout)
            heartbeat(args.atm, env, BARE_MEMBER, "active-tool-use", args.timeout)
            herdr_send = send_queue(args.atm, env, HERDR_MEMBER, "aq3-herdr-must-not-claim", args.timeout)
            bare_send = send_queue(args.atm, env, BARE_MEMBER, "aq3-bare-fifo", args.timeout)
            herdr_idle = heartbeat(args.atm, env, HERDR_MEMBER, "idle", args.timeout)
            bare_idle = heartbeat(args.atm, env, BARE_MEMBER, "idle", args.timeout)
            herdr_get = queue_get(args.atm, env, HERDR_MEMBER, args.timeout)
            bare_get = queue_get(args.atm, env, BARE_MEMBER, args.timeout)
            record["scenario_3_skip_herdr_bare_cli_precheck"] = {
                "herdr_send": herdr_send,
                "bare_cli_send": bare_send,
                "herdr_idle_heartbeat": herdr_idle,
                "bare_cli_idle_heartbeat": bare_idle,
                "herdr_queue_get": herdr_get,
                "bare_cli_queue_get": bare_get,
                "herdr_not_claimed": herdr_get["stdout"] == "",
                "bare_cli_uses_fifo_get": bare_send["body"] in bare_get["stdout"],
            }

            scenarios = [record["scenario_1_idle_transition_drain"], record["scenario_2_recovery_sweep_after_restart"], record["scenario_3_skip_herdr_bare_cli_precheck"]]
            record["status"] = "pass" if all(
                (
                    scenarios[0]["exactly_one_nudge_per_idle"],
                    scenarios[1]["recovery_dispatch_confirmed"],
                    scenarios[2]["herdr_not_claimed"],
                    scenarios[2]["bare_cli_uses_fifo_get"],
                )
            ) else "fail"
    except Exception as error:  # noqa: BLE001 - evidence must retain the failure
        record["status"] = "fail"
        record["error"] = f"{type(error).__name__}: {error}"
    finally:
        stop_daemon(daemon_process)
        stop_tmux_session(tmux_session)
        record["finished_at_ns"] = time.time_ns()
    return record


def write_evidence(args: argparse.Namespace, record: dict[str, Any]) -> tuple[Path, Path]:
    evidence_dir = args.evidence_dir or ROOT / "docs" / "plans" / "phase-aq" / "evidence" / "AQ3"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    json_path = evidence_dir / f"idle-drain-sweep-{args.host}.json"
    markdown_path = evidence_dir / f"idle-drain-sweep-{args.host}.md"
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
        "# AQ3 idle-drain and recovery-sweep evidence",
        "",
        f"Host: `{args.host}`",
        f"Commit: `{payload['commit']}`",
        f"Status: **{record['status'].upper()}**",
        "",
    ]
    if record["status"] == "blocked_ambient_daemon":
        lines += [
            f"Blocked by ambient `atm-daemon` pid(s) `{record['ambient_daemon_pids']}`.",
            "The runner fails closed because ATM's daemon owner and database scope is OS-account scoped.",
            "Run on a dedicated account with no ambient daemon for positive-path evidence.",
        ]
    elif record["status"] == "pass":
        lines += [
            "## Scenario 1 — idle-transition drain",
            "",
            "Three queue messages were sent to a tmux-classified member; each AQ2.5 Stop-hook idle heartbeat produced exactly one nudge.",
            "",
            "## Scenario 2 — restart recovery sweep",
            "",
            "A pending tmux message was dispatched after an owned daemon restart without another idle heartbeat.",
            "",
            "## Scenario 3 — Herdr and bare-CLI pre-check",
            "",
            "The Herdr queue-get remained empty while the bare-CLI item was returned by its FIFO get, proving AQ3 did not claim either channel.",
        ]
    else:
        lines += ["The runner recorded a failure; inspect the JSON record for bounded stdout/stderr diagnostics."]
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def main() -> int:
    args = parse_args()
    if args.timeout <= 0 or args.recovery_timeout <= 0:
        raise SystemExit("--timeout and --recovery-timeout must be positive")
    if not args.daemon.is_file():
        raise SystemExit(f"owned daemon binary does not exist: {args.daemon}")
    if not args.atm.is_file():
        raise SystemExit(f"matched atm CLI binary does not exist: {args.atm}")
    record = run_scenario(args)
    json_path, markdown_path = write_evidence(args, record)
    print(f"{record['status'].upper()} AQ3 idle-drain/sweep evidence: {json_path}")
    print(f"transcript: {markdown_path}")
    return 0 if record["status"] in ("pass", "blocked_ambient_daemon") else 1


if __name__ == "__main__":
    raise SystemExit(main())

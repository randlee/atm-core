#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from analyze_logs import analyze_log_text
from fixtures import create_clean_room_fixture
from fixtures import current_binary_sha
from fixtures import repo_root
from fixtures import smoke_env


ROW_MAP: dict[str, list[tuple[str, str]]] = {
    "fast": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
    "normal": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("Z1-007", "retained CLI validation and recovery guidance"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
    "thorough": [
        ("Z1-001", "build approved smoke baseline"),
        ("Z1-002", "clean-room daemon/runtime bring-up"),
        ("Z1-003", "retained team/member inspection on clean-room baseline"),
        ("Z1-004", "empty-mailbox retained CLI surface"),
        ("Z1-005", "first clean-room send to config-defined recipient"),
        ("Z1-006", "degraded notification after durable send"),
        ("Z1-007", "retained CLI validation and recovery guidance"),
        ("Z1-008", "copied-state durable baseline bring-up"),
        ("Z1-009", "reconcile/runtime retry-visible smoke coverage"),
        ("FAST-LOG-001", "expected happy-path retained events are present"),
        ("FAST-LOG-002", "retained logs contain no warnings or errors"),
    ],
}

FAST_TEAM = "z19-team"
FAST_OPERATOR = "z19-operator"
FAST_RECIPIENT = "z19-recipient"


@dataclass
class SmokeRow:
    id: str
    flow: str
    verdict: str = "SKIP"
    notes: str = ""
    observed_behavior: str | None = None
    expected_behavior: str | None = None
    likely_root_cause: str | None = None
    artifact_pointer: str | None = None

    def to_payload(self) -> dict[str, object]:
        payload = asdict(self)
        return {key: value for key, value in payload.items() if value is not None}


@dataclass(frozen=True)
class CommandResult:
    argv: list[str]
    returncode: int
    stdout: str
    stderr: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Shared Phase Z smoke runner.")
    parser.add_argument("level", choices=sorted(ROW_MAP))
    parser.add_argument("--binary-sha", default=None)
    parser.add_argument("--write-artifacts", action="store_true")
    parser.add_argument(
        "--status",
        default=None,
        help="Override the runner status label. Later smoke sprints should not need this.",
    )
    return parser.parse_args()


def render_markdown(payload_path: Path, write_artifacts: bool) -> None:
    command = [
        sys.executable,
        str(repo_root() / "scripts" / "smoke" / "render_report.py"),
        str(payload_path),
    ]
    if write_artifacts:
        command.append("--write-artifacts")
    subprocess.run(command, check=True)


def scaffold_payload(level: str, status: str, binary_sha: str) -> dict[str, object]:
    rows = [
        SmokeRow(
            id=row_id,
            flow=flow,
            verdict="SKIP",
            notes="scaffold-only runner contract; execution steps land in later smoke sprints",
        ).to_payload()
        for row_id, flow in ROW_MAP[level]
    ]
    return {
        "level": level,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "binary_sha": binary_sha,
        "duration_secs": 0.0,
        "status": status,
        "rows": rows,
        "summary": {
            "pass": 0,
            "fail": 0,
            "skip": len(rows),
        },
    }


def build_release_binaries(root: Path) -> None:
    subprocess.run(
        ["cargo", "build", "--release", "-p", "agent-team-mail", "-p", "atm-daemon"],
        cwd=root,
        check=True,
    )


def run_atm(
    root: Path,
    fixture_env: dict[str, str],
    workspace_dir: Path,
    *args: str,
    identity: str | None = None,
    expect_success: bool = True,
) -> CommandResult:
    env = fixture_env.copy()
    if identity is not None:
        env["ATM_IDENTITY"] = identity
    command = [str(root / "target" / "release" / "atm"), *args]
    with tempfile.NamedTemporaryFile("w+", encoding="utf-8") as stdout_handle:
        with tempfile.NamedTemporaryFile("w+", encoding="utf-8") as stderr_handle:
            completed = subprocess.run(
                command,
                cwd=workspace_dir,
                env=env,
                text=True,
                stdout=stdout_handle,
                stderr=stderr_handle,
                check=False,
            )
            stdout_handle.seek(0)
            stderr_handle.seek(0)
            result = CommandResult(
                argv=command,
                returncode=completed.returncode,
                stdout=stdout_handle.read(),
                stderr=stderr_handle.read(),
            )
    if expect_success and result.returncode != 0:
        raise RuntimeError(command_failure_message(result))
    return result


def parse_json_output(result: CommandResult) -> dict[str, object]:
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"failed to decode JSON output from {' '.join(result.argv)}: {exc}\nstdout={result.stdout}\nstderr={result.stderr}"
        ) from exc


def command_failure_message(result: CommandResult) -> str:
    return (
        f"command failed: {' '.join(result.argv)}\n"
        f"exit={result.returncode}\n"
        f"stdout={result.stdout}\n"
        f"stderr={result.stderr}"
    )


def fail_row(
    row: SmokeRow,
    *,
    observed: str,
    expected: str,
    root_cause: str,
    artifact: str,
    notes: str,
) -> None:
    row.verdict = "FAIL"
    row.notes = notes
    row.observed_behavior = observed
    row.expected_behavior = expected
    row.likely_root_cause = root_cause
    row.artifact_pointer = artifact


def pass_row(row: SmokeRow, notes: str) -> None:
    row.verdict = "PASS"
    row.notes = notes


def stop_daemon(pid: int) -> None:
    if os.name != "posix":
        raise RuntimeError(
            "just smoke fast currently supports only POSIX daemon shutdown semantics"
        )
    os.kill(pid, signal.SIGTERM)
    deadline = time.time() + 5.0
    while time.time() < deadline:
        if not process_is_alive(pid):
            return
        time.sleep(0.05)
    raise RuntimeError(f"daemon pid {pid} did not exit after SIGTERM")


def process_is_alive(pid: int) -> bool:
    status = subprocess.run(
        ["ps", "-p", str(pid), "-o", "stat="],
        text=True,
        capture_output=True,
        check=False,
    )
    if status.returncode != 0:
        return False
    state = status.stdout.strip()
    return bool(state) and "Z" not in state


def run_fast(binary_sha: str) -> dict[str, object]:
    root = repo_root()
    started = time.perf_counter()
    rows = {
        row_id: SmokeRow(id=row_id, flow=flow)
        for row_id, flow in ROW_MAP["fast"]
    }
    fixture = create_clean_room_fixture(
        prefix="z19-smoke-fast.",
        team_name=FAST_TEAM,
        operator=FAST_OPERATOR,
        recipient=FAST_RECIPIENT,
    )
    base_env = smoke_env(fixture, identity=FAST_OPERATOR, root=root)
    log_path = fixture.log_dir / "atm.log.jsonl"
    doctor_payload: dict[str, object] | None = None
    send_no_ack_payload: dict[str, object] | None = None
    send_ack_payload: dict[str, object] | None = None
    read_payload: dict[str, object] | None = None
    ack_payload: dict[str, object] | None = None
    empty_log_snapshot: dict[str, object] | None = None
    daemon_pid: int | None = None
    status = "passed"

    try:
        build_release_binaries(root)
        pass_row(rows["Z1-001"], "release smoke binaries built successfully")

        doctor_payload = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "doctor", "--json")
        )
        runtime_status = doctor_payload.get("runtime_status") or {}
        daemon_pid = runtime_status.get("singleton_owner_pid")  # type: ignore[assignment]
        if (
            doctor_payload.get("summary", {}).get("status") == "healthy"
            and runtime_status.get("readiness") == "ready"
        ):
            pass_row(
                rows["Z1-002"],
                "doctor auto-started the daemon and reported healthy readiness on the clean-room baseline",
            )
        else:
            fail_row(
                rows["Z1-002"],
                observed=json.dumps(doctor_payload, indent=2),
                expected="doctor summary healthy and runtime_status.readiness=ready",
                root_cause="daemon bootstrap or readiness projection did not reach the accepted healthy baseline",
                artifact="doctor --json",
                notes="clean-room daemon/runtime bring-up did not close cleanly",
            )
            status = "failed"

        run_atm(
            root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            FAST_TEAM,
            FAST_OPERATOR,
            "--json",
        )
        run_atm(
            root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            FAST_TEAM,
            FAST_RECIPIENT,
            "--json",
        )
        teams_payload = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "teams", "--json")
        )
        members_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "members",
                "--team",
                FAST_TEAM,
                "--json",
            )
        )
        member_names = [member["name"] for member in members_payload.get("members", [])]
        team_entries = teams_payload.get("teams", [])
        team_names = [
            entry["name"] if isinstance(entry, dict) and "name" in entry else entry
            for entry in team_entries
        ]
        if FAST_TEAM in team_names and member_names == [
            FAST_OPERATOR,
            FAST_RECIPIENT,
        ]:
            pass_row(
                rows["Z1-003"],
                "teams and members returned the retained clean-room roster after explicit add-member setup",
            )
        else:
            fail_row(
                rows["Z1-003"],
                observed=json.dumps(
                    {"teams": teams_payload, "members": members_payload},
                    indent=2,
                ),
                expected="teams contains z19-team and members lists z19-operator and z19-recipient",
                root_cause="retained roster inspection did not reflect the accepted clean-room setup path",
                artifact="teams --json / members --team z19-team --json",
                notes="clean-room retained roster inspection failed",
            )
            status = "failed"

        list_payload = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "list", "--json")
        )
        read_empty_payload = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "read", "--all", "--json")
        )
        clear_payload = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "clear", "--json")
        )
        empty_log_snapshot = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "log", "snapshot", "--json")
        )
        if (
            list_payload.get("count") == 0
            and read_empty_payload.get("count") == 0
            and clear_payload.get("removed_total") == 0
            and isinstance(empty_log_snapshot.get("records"), list)
        ):
            pass_row(
                rows["Z1-004"],
                "list/read/clear/log snapshot all succeeded on the clean-room empty-mailbox baseline",
            )
        else:
            fail_row(
                rows["Z1-004"],
                observed=json.dumps(
                    {
                        "list": list_payload,
                        "read": read_empty_payload,
                        "clear": clear_payload,
                        "log_snapshot": empty_log_snapshot,
                    },
                    indent=2,
                ),
                expected="empty-mailbox list/read/clear succeed and log snapshot returns records without mailbox failures",
                root_cause="one or more retained empty-mailbox CLI surfaces diverged from the accepted baseline contract",
                artifact="list/read/clear/log snapshot --json",
                notes="empty-mailbox retained CLI smoke lane failed",
            )
            status = "failed"

        send_no_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "send",
                FAST_RECIPIENT,
                "fast smoke no ack",
                "--json",
            )
        )
        send_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "send",
                FAST_RECIPIENT,
                "fast smoke requires ack",
                "--requires-ack",
                "--json",
            )
        )
        ack_required_message_id = str(send_ack_payload["message_id"])
        read_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "read",
                FAST_RECIPIENT,
                "--team",
                FAST_TEAM,
                "--all",
                "--message-id",
                ack_required_message_id,
                "--json",
            )
        )
        ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "ack",
                ack_required_message_id,
                "fast smoke ack reply",
                "--team",
                FAST_TEAM,
                "--as",
                FAST_RECIPIENT,
                "--json",
            )
        )
        send_ok = (
            send_no_ack_payload.get("outcome") == "sent"
            and send_no_ack_payload.get("requires_ack") is False
            and send_ack_payload.get("outcome") == "sent"
            and send_ack_payload.get("requires_ack") is True
        )
        read_ok = read_payload.get("selected_message_id") == ack_required_message_id
        ack_ok = ack_payload.get("message_id") == ack_required_message_id
        if send_ok and read_ok and ack_ok:
            pass_row(
                rows["Z1-005"],
                "both send modes succeeded; the ack-required message was read from the recipient mailbox and acknowledged successfully",
            )
        else:
            fail_row(
                rows["Z1-005"],
                observed=json.dumps(
                    {
                        "send_no_ack": send_no_ack_payload,
                        "send_ack": send_ack_payload,
                        "read": read_payload,
                        "ack": ack_payload,
                    },
                    indent=2,
                ),
                expected="non-ack send succeeds; ack-required send succeeds; recipient can read and ack the durable message",
                root_cause="the clean-room send/read/ack happy path did not complete end-to-end for both send modes",
                artifact="send/read/ack JSON outputs",
                notes="fast send/read/ack lane failed",
            )
            status = "failed"

        if daemon_pid is not None:
            stop_daemon(int(daemon_pid))
    except Exception as exc:
        status = "failed"
        first_pending = next(
            (row for row in rows.values() if row.verdict == "SKIP"),
            None,
        )
        if first_pending is not None:
            fail_row(
                first_pending,
                observed=str(exc),
                expected="the active fast smoke step succeeds",
                root_cause="runner-level failure interrupted the fast smoke lane",
                artifact="runner exception",
                notes="runner aborted during fast smoke execution",
            )
    finally:
        analysis: dict[str, object] | None = None
        if log_path.exists():
            log_text = log_path.read_text(encoding="utf-8")
            analysis_result = analyze_log_text(
                log_text,
                [
                    '"action":"send"',
                    '"action":"read"',
                    '"action":"ack"',
                    '"outcome":"delivery_policy.new_message.primary_nudge"',
                    '"outcome":"delivery_policy.ack_reply.delivered"',
                    '"action":"shutdown_completed"',
                ],
            )
            analysis = {
                "passed": analysis_result.passed,
                "expected_events": analysis_result.expected_events,
                "missing_events": analysis_result.missing_events,
                "warning_records": analysis_result.warning_records,
                "error_records": analysis_result.error_records,
            }
            if analysis_result.missing_events:
                fail_row(
                    rows["FAST-LOG-001"],
                    observed=json.dumps(analysis, indent=2),
                    expected="retained log contains send/read/ack/shutdown events plus delivery_policy.new_message.primary_nudge and delivery_policy.ack_reply.delivered",
                    root_cause="required debug/verbose smoke-fast events were not emitted into the retained log",
                    artifact=str(log_path),
                    notes="required fast retained-log events were missing",
                )
                status = "failed"
            else:
                pass_row(
                    rows["FAST-LOG-001"],
                    "retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events",
                )
            if analysis_result.warning_records or analysis_result.error_records:
                fail_row(
                    rows["FAST-LOG-002"],
                    observed=json.dumps(analysis, indent=2),
                    expected="retained log contains no warning or error records on a healthy fast smoke run",
                    root_cause="one or more healthy-path events are still being emitted at warn/error severity",
                    artifact=str(log_path),
                    notes="retained log severity gate failed",
                )
                status = "failed"
            else:
                pass_row(
                    rows["FAST-LOG-002"],
                    "retained log contained no warning or error records during the healthy fast smoke run",
                )
        else:
            fail_row(
                rows["FAST-LOG-001"],
                observed="retained log path was not created",
                expected="retained log file exists and can be analyzed",
                root_cause="shared observability log bootstrap did not create the expected log artifact",
                artifact=str(log_path),
                notes="retained log file was missing",
            )
            fail_row(
                rows["FAST-LOG-002"],
                observed="retained log path was not created",
                expected="retained log file exists and can be analyzed for warning/error-free execution",
                root_cause="shared observability log bootstrap did not create the expected log artifact",
                artifact=str(log_path),
                notes="retained log file was missing",
            )
            status = "failed"
        if status == "passed":
            shutil.rmtree(fixture.root, ignore_errors=True)

    ordered_rows = [rows[row_id].to_payload() for row_id, _ in ROW_MAP["fast"]]
    summary = {"pass": 0, "fail": 0, "skip": 0}
    for row in ordered_rows:
        verdict = row["verdict"].lower()
        summary[verdict] += 1
    return {
        "level": "fast",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "binary_sha": binary_sha,
        "duration_secs": round(time.perf_counter() - started, 3),
        "status": status,
        "rows": ordered_rows,
        "summary": summary,
    }


def build_payload(level: str, status: str | None, binary_sha: str) -> dict[str, object]:
    if level == "fast":
        return run_fast(binary_sha)
    scaffold_status = status or "scaffold-only"
    return scaffold_payload(level, scaffold_status, binary_sha)


def render_stdout_summary(payload: dict) -> str:
    summary = payload["summary"]
    lines = [
        f"smoke level: {payload['level']}",
        f"runner status: {payload['status']}",
        f"binary sha: {payload['binary_sha']}",
        f"duration secs: {payload['duration_secs']}",
        f"summary: pass={summary['pass']} fail={summary['fail']} skip={summary['skip']}",
    ]
    for row in payload["rows"]:
        if row["verdict"] != "PASS":
            lines.append(f"{row['id']}: {row['verdict']} - {row['notes']}")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    binary_sha = args.binary_sha or current_binary_sha()
    payload = build_payload(args.level, args.status, binary_sha)
    if args.write_artifacts:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".json", delete=False, encoding="utf-8"
        ) as handle:
            json.dump(payload, handle, indent=2)
            handle.write("\n")
            temp_payload = Path(handle.name)
        try:
            render_markdown(temp_payload, write_artifacts=True)
        finally:
            temp_payload.unlink(missing_ok=True)
    else:
        print(json.dumps(payload, indent=2))
    print(render_stdout_summary(payload))
    return 1 if payload["summary"]["fail"] > 0 else 0


if __name__ == "__main__":
    raise SystemExit(main())

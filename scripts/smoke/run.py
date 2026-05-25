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
from fixtures import clone_fixture
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
NORMAL_TEAM = "z20-team"
NORMAL_OPERATOR = "z20-operator"
NORMAL_RECIPIENT = "z20-recipient"
THOROUGH_TEAM = "z21-team"
THOROUGH_OPERATOR = "z21-operator"
THOROUGH_RECIPIENT = "z21-recipient"


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


def failure_mentions(result: CommandResult, needle: str) -> bool:
    lowered = needle.lower()
    return lowered in result.stdout.lower() or lowered in result.stderr.lower()


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


def run_clean_room_lane(
    *,
    level: str,
    binary_sha: str,
    team: str,
    operator: str,
    recipient: str,
    include_validation_check: bool,
) -> dict[str, object]:
    root = repo_root()
    started = time.perf_counter()
    rows = {
        row_id: SmokeRow(id=row_id, flow=flow)
        for row_id, flow in ROW_MAP[level]
    }
    fixture = create_clean_room_fixture(
        prefix=f"{team}-{level}.",
        team_name=team,
        operator=operator,
        recipient=recipient,
    )
    base_env = smoke_env(fixture, identity=operator, root=root)
    log_path = fixture.log_dir / "atm.log.jsonl"
    doctor_payload: dict[str, object] | None = None
    send_no_ack_payload: dict[str, object] | None = None
    send_ack_payload: dict[str, object] | None = None
    read_payload: dict[str, object] | None = None
    ack_payload: dict[str, object] | None = None
    pending_ack_list_payload: dict[str, object] | None = None
    post_ack_list_payload: dict[str, object] | None = None
    post_clear_read_payload: dict[str, object] | None = None
    post_activity_log_snapshot: dict[str, object] | None = None
    invalid_ack_result: CommandResult | None = None
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
            team,
            operator,
            "--json",
        )
        run_atm(
            root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            team,
            recipient,
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
                team,
                "--json",
            )
        )
        member_names = [member["name"] for member in members_payload.get("members", [])]
        team_entries = teams_payload.get("teams", [])
        team_names = [
            entry["name"] if isinstance(entry, dict) and "name" in entry else entry
            for entry in team_entries
        ]
        if team in team_names and member_names == [
            operator,
            recipient,
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
                expected=f"teams contains {team} and members lists {operator} and {recipient}",
                root_cause="retained roster inspection did not reflect the accepted clean-room setup path",
                artifact=f"teams --json / members --team {team} --json",
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
                recipient,
                f"{level} smoke no ack",
                "--json",
            )
        )
        send_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "send",
                recipient,
                f"{level} smoke requires ack",
                "--requires-ack",
                "--json",
            )
        )
        ack_required_message_id = str(send_ack_payload["message_id"])
        if include_validation_check:
            pending_ack_list_payload = parse_json_output(
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "list",
                    recipient,
                    "--team",
                    team,
                    "--pending-ack",
                    "--json",
                )
            )
        read_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "read",
                recipient,
                "--team",
                team,
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
                f"{level} smoke ack reply",
                "--team",
                team,
                "--as",
                recipient,
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
                notes="core send/read/ack lane failed",
            )
            status = "failed"

        if include_validation_check:
            post_ack_list_payload = parse_json_output(
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "list",
                    recipient,
                    "--team",
                    team,
                    "--all",
                    "--json",
                )
            )
            clear_payload = parse_json_output(
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "clear",
                    recipient,
                    "--team",
                    team,
                    "--json",
                )
            )
            post_clear_read_payload = parse_json_output(
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "read",
                    recipient,
                    "--team",
                    team,
                    "--all",
                    "--json",
                )
            )
            post_activity_log_snapshot = parse_json_output(
                run_atm(root, base_env, fixture.workspace_dir, "log", "snapshot", "--json")
            )
            invalid_ack_result = run_atm(
                root,
                smoke_env(fixture, identity=recipient, root=root),
                fixture.workspace_dir,
                "ack",
                "",
                "invalid ack from smoke normal",
                "--json",
                expect_success=False,
            )

            pending_rows = pending_ack_list_payload.get("rows", [])
            pending_ok = (
                pending_ack_list_payload.get("count") == 1
                and pending_ack_list_payload.get("bucket_counts", {}).get("pending_ack", 0) >= 1
                and any(
                    row.get("message_id") == ack_required_message_id
                    for row in pending_rows
                    if isinstance(row, dict)
                )
            )
            post_ack_ok = (
                post_ack_list_payload.get("count", 0) >= 2
                and clear_payload.get("removed_total") == 1
                and clear_payload.get("remaining_total") == 1
                and post_clear_read_payload.get("count") == 1
                and isinstance(post_activity_log_snapshot.get("records"), list)
                and invalid_ack_result.returncode != 0
                and "invalid message id" in invalid_ack_result.stderr.lower()
            )
            if pending_ok and post_ack_ok:
                pass_row(
                    rows["Z1-007"],
                    "pending-ack inspection, post-ack mailbox clear/re-read, log snapshot, and invalid-ack recovery guidance all behaved as expected",
                )
            else:
                fail_row(
                    rows["Z1-007"],
                    observed=json.dumps(
                        {
                            "pending_ack_list": pending_ack_list_payload,
                            "post_ack_list": post_ack_list_payload,
                            "clear": clear_payload,
                            "post_clear_read": post_clear_read_payload,
                            "post_activity_log_snapshot": post_activity_log_snapshot,
                            "invalid_ack": {
                                "returncode": invalid_ack_result.returncode,
                                "stdout": invalid_ack_result.stdout,
                                "stderr": invalid_ack_result.stderr,
                            },
                        },
                        indent=2,
                    ),
                    expected="pending-ack listing shows the durable ack-required message, post-ack clear removes exactly the acknowledged message, re-read leaves the non-ack message, log snapshot succeeds, and invalid ack fails with explicit recovery guidance",
                    root_cause="one or more retained mailbox validation or recovery surfaces diverged from the accepted normal smoke contract",
                    artifact="list/clear/read/log snapshot/invalid ack outputs",
                    notes="normal validation and recovery guidance lane failed",
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
                expected="the active smoke step succeeds",
                root_cause="runner-level failure interrupted the clean-room smoke lane",
                artifact="runner exception",
                notes="runner aborted during clean-room smoke execution",
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

    ordered_rows = [rows[row_id].to_payload() for row_id, _ in ROW_MAP[level]]
    summary = {"pass": 0, "fail": 0, "skip": 0}
    for row in ordered_rows:
        verdict = row["verdict"].lower()
        summary[verdict] += 1
    return {
        "level": level,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "binary_sha": binary_sha,
        "duration_secs": round(time.perf_counter() - started, 3),
        "status": status,
        "rows": ordered_rows,
        "summary": summary,
    }


def run_thorough(binary_sha: str) -> dict[str, object]:
    root = repo_root()
    started = time.perf_counter()
    rows = {
        row_id: SmokeRow(id=row_id, flow=flow)
        for row_id, flow in ROW_MAP["thorough"]
    }
    fixture = create_clean_room_fixture(
        prefix="z21-smoke-thorough.",
        team_name=THOROUGH_TEAM,
        operator=THOROUGH_OPERATOR,
        recipient=THOROUGH_RECIPIENT,
    )
    base_env = smoke_env(fixture, identity=THOROUGH_OPERATOR, root=root)
    log_path = fixture.log_dir / "atm.log.jsonl"
    status = "passed"
    daemon_pid: int | None = None
    copied_daemon_pid: int | None = None

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
            THOROUGH_TEAM,
            THOROUGH_OPERATOR,
            "--json",
        )
        run_atm(
            root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            THOROUGH_TEAM,
            THOROUGH_RECIPIENT,
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
                THOROUGH_TEAM,
                "--json",
            )
        )
        backup_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "teams",
                "backup",
                THOROUGH_TEAM,
                "--json",
            )
        )
        restore_plan_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "teams",
                "restore",
                THOROUGH_TEAM,
                "--from",
                str(backup_payload["backup_path"]),
                "--dry-run",
                "--json",
            )
        )
        member_names = [member["name"] for member in members_payload.get("members", [])]
        team_names = [
            entry["name"] if isinstance(entry, dict) and "name" in entry else entry
            for entry in teams_payload.get("teams", [])
        ]
        roster_ok = (
            THOROUGH_TEAM in team_names
            and member_names == [THOROUGH_OPERATOR, THOROUGH_RECIPIENT]
            and Path(str(backup_payload["backup_path"])).exists()
            and restore_plan_payload.get("team") == THOROUGH_TEAM
        )
        if roster_ok:
            pass_row(
                rows["Z1-003"],
                "teams, members, backup, and restore dry-run all succeeded on the clean-room retained/admin baseline",
            )
        else:
            fail_row(
                rows["Z1-003"],
                observed=json.dumps(
                    {
                        "teams": teams_payload,
                        "members": members_payload,
                        "backup": backup_payload,
                        "restore_plan": restore_plan_payload,
                    },
                    indent=2,
                ),
                expected="teams/members return the clean-room roster and teams backup/restore dry-run succeed on the same team",
                root_cause="one or more retained team-admin surfaces diverged from the accepted clean-room setup path",
                artifact="teams --json / members --team ... --json / teams backup / teams restore --dry-run",
                notes="clean-room retained roster and team-admin inspection failed",
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
        help_overview = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "help", "--json")
        )
        help_send = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "help", "send", "--json")
        )
        empty_surface_ok = (
            list_payload.get("count") == 0
            and read_empty_payload.get("count") == 0
            and clear_payload.get("removed_total") == 0
            and isinstance(empty_log_snapshot.get("records"), list)
            and help_overview.get("kind") == "overview"
            and help_send.get("kind") == "command_help"
        )
        if empty_surface_ok:
            pass_row(
                rows["Z1-004"],
                "list/read/clear/log snapshot plus ATM help overview/send guidance all succeeded on the clean-room baseline",
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
                        "help_overview": help_overview,
                        "help_send": help_send,
                    },
                    indent=2,
                ),
                expected="empty-mailbox mailbox/log/help surfaces all succeed on the clean-room baseline",
                root_cause="one or more retained mailbox, log, or help surfaces diverged from the accepted baseline contract",
                artifact="list/read/clear/log snapshot/help JSON outputs",
                notes="empty-mailbox retained CLI and help surface failed",
            )
            status = "failed"

        send_no_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "send",
                THOROUGH_RECIPIENT,
                "thorough smoke no ack",
                "--json",
            )
        )
        send_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "send",
                THOROUGH_RECIPIENT,
                "thorough smoke requires ack",
                "--requires-ack",
                "--json",
            )
        )
        ack_required_message_id = str(send_ack_payload["message_id"])
        pending_ack_list_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "list",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--pending-ack",
                "--json",
            )
        )
        read_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "read",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
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
                "thorough smoke ack reply",
                "--team",
                THOROUGH_TEAM,
                "--as",
                THOROUGH_RECIPIENT,
                "--json",
            )
        )
        post_ack_list_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "list",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--all",
                "--json",
            )
        )
        clear_after_ack_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "clear",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--json",
            )
        )
        post_clear_read_payload = parse_json_output(
            run_atm(
                root,
                base_env,
                fixture.workspace_dir,
                "read",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--all",
                "--json",
            )
        )
        post_activity_log_snapshot = parse_json_output(
            run_atm(root, base_env, fixture.workspace_dir, "log", "snapshot", "--json")
        )
        happy_path_ok = (
            send_no_ack_payload.get("outcome") == "sent"
            and send_no_ack_payload.get("requires_ack") is False
            and send_ack_payload.get("outcome") == "sent"
            and send_ack_payload.get("requires_ack") is True
            and pending_ack_list_payload.get("count") == 1
            and read_payload.get("selected_message_id") == ack_required_message_id
            and ack_payload.get("message_id") == ack_required_message_id
            and clear_after_ack_payload.get("removed_total") == 1
            and clear_after_ack_payload.get("remaining_total") == 1
            and post_clear_read_payload.get("count") == 1
            and isinstance(post_activity_log_snapshot.get("records"), list)
        )
        if happy_path_ok:
            pass_row(
                rows["Z1-005"],
                "both send modes, pending-ack inspection, recipient read/ack, and post-ack clear/re-read all succeeded on the clean-room baseline",
            )
        else:
            fail_row(
                rows["Z1-005"],
                observed=json.dumps(
                    {
                        "send_no_ack": send_no_ack_payload,
                        "send_ack": send_ack_payload,
                        "pending_ack_list": pending_ack_list_payload,
                        "read": read_payload,
                        "ack": ack_payload,
                        "post_ack_list": post_ack_list_payload,
                        "clear": clear_after_ack_payload,
                        "post_clear_read": post_clear_read_payload,
                        "post_activity_log_snapshot": post_activity_log_snapshot,
                    },
                    indent=2,
                ),
                expected="happy-path send/read/ack plus post-ack mailbox surfaces all succeed on the clean-room baseline",
                root_cause="the broad clean-room send/read/ack mailbox flow diverged before the thorough lane could prove the expected operator behavior",
                artifact="send/list/read/ack/clear/log snapshot JSON outputs",
                notes="thorough clean-room send/read/ack lane failed",
            )
            status = "failed"

        if daemon_pid is not None:
            stop_daemon(int(daemon_pid))
            daemon_pid = None

        clean_room_log_text = log_path.read_text(encoding="utf-8") if log_path.exists() else ""
        analysis_result = analyze_log_text(
            clean_room_log_text,
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
                root_cause="required debug/verbose healthy-path events were not emitted into the retained log before the negative-path inventory began",
                artifact=str(log_path),
                notes="required fast retained-log events were missing",
            )
            status = "failed"
        else:
            pass_row(
                rows["FAST-LOG-001"],
                "retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events before negative-path execution",
            )
        if analysis_result.warning_records or analysis_result.error_records:
            fail_row(
                rows["FAST-LOG-002"],
                observed=json.dumps(analysis, indent=2),
                expected="retained log contains no warning or error records on the healthy-path portion of the thorough run",
                root_cause="one or more healthy-path events are still being emitted at warn/error severity before negative-path coverage begins",
                artifact=str(log_path),
                notes="retained log severity gate failed",
            )
            status = "failed"
        else:
            pass_row(
                rows["FAST-LOG-002"],
                "retained log contained no warning or error records during the healthy-path portion of the thorough run",
            )

        error_matrix = [
            (
                "send_invalid_target",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "send",
                    "../evil",
                    "bad target",
                    "--json",
                    expect_success=False,
                ),
                "agent name",
            ),
            (
                "read_invalid_target",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "read",
                    "../evil",
                    "--json",
                    expect_success=False,
                ),
                "agent name",
            ),
            (
                "ack_invalid_message_id",
                run_atm(
                    root,
                    smoke_env(fixture, identity=THOROUGH_RECIPIENT, root=root),
                    fixture.workspace_dir,
                    "ack",
                    "",
                    "invalid ack from smoke thorough",
                    "--json",
                    expect_success=False,
                ),
                "invalid message id",
            ),
            (
                "list_invalid_target",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "list",
                    "../evil",
                    "--json",
                    expect_success=False,
                ),
                "agent name",
            ),
            (
                "clear_invalid_target",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "clear",
                    "../evil",
                    "--json",
                    expect_success=False,
                ),
                "agent name",
            ),
            (
                "log_filter_requires_predicate",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "log",
                    "filter",
                    "--json",
                    expect_success=False,
                ),
                "requires at least one",
            ),
            (
                "doctor_invalid_team",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "doctor",
                    "--team",
                    "../evil",
                    "--json",
                    expect_success=False,
                ),
                "team name",
            ),
            (
                "teams_invalid_team",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "teams",
                    "add-member",
                    "../evil",
                    "bad-member",
                    "--json",
                    expect_success=False,
                ),
                "team name",
            ),
            (
                "members_invalid_team",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "members",
                    "--team",
                    "../evil",
                    "--json",
                    expect_success=False,
                ),
                "team name",
            ),
            (
                "help_unknown_target",
                run_atm(
                    root,
                    base_env,
                    fixture.workspace_dir,
                    "help",
                    "no-such-target",
                    "--json",
                    expect_success=False,
                ),
                "unknown help topic or subcommand",
            ),
        ]
        error_matrix_failures = [
            {
                "name": name,
                "returncode": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "expected_substring": needle,
            }
            for name, result, needle in error_matrix
            if result.returncode == 0 or not failure_mentions(result, needle)
        ]
        if error_matrix_failures:
            fail_row(
                rows["Z1-007"],
                observed=json.dumps(error_matrix_failures, indent=2),
                expected="each CLI validation path fails closed with an actionable typed error instead of mutating state silently",
                root_cause="one or more command-entry validation surfaces did not reject the common error path with the expected recovery guidance",
                artifact="negative-path CLI stderr/stdout matrix",
                notes="thorough CLI validation and recovery guidance matrix failed",
            )
            status = "failed"
        else:
            pass_row(
                rows["Z1-007"],
                "send/read/ack/list/clear/log/doctor/teams/members/help common error paths all failed closed with explicit actionable guidance",
            )

        copied_fixture = clone_fixture(
            fixture,
            prefix="z21-smoke-copied.",
            clear_logs=True,
        )
        copied_env = smoke_env(copied_fixture, identity=THOROUGH_OPERATOR, root=root)
        legacy_inbox_path = copied_fixture.team_dir / "inboxes" / f"{THOROUGH_RECIPIENT}.json"
        legacy_inbox_path.write_text(
            json.dumps(
                [
                    {
                        "from": f"{THOROUGH_OPERATOR}@{THOROUGH_TEAM}",
                        "message": "legacy array mailbox placeholder",
                    }
                ],
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        copied_doctor = parse_json_output(
            run_atm(root, copied_env, copied_fixture.workspace_dir, "doctor", "--json")
        )
        copied_daemon_pid = copied_doctor.get("runtime_status", {}).get("singleton_owner_pid")  # type: ignore[assignment]
        copied_list = parse_json_output(
            run_atm(
                root,
                copied_env,
                copied_fixture.workspace_dir,
                "list",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--all",
                "--json",
            )
        )
        copied_send_result = run_atm(
            root,
            copied_env,
            copied_fixture.workspace_dir,
            "send",
            THOROUGH_RECIPIENT,
            "thorough copied-state degraded send",
            "--requires-ack",
            "--json",
        )
        copied_send = parse_json_output(copied_send_result)
        copied_read = parse_json_output(
            run_atm(
                root,
                copied_env,
                copied_fixture.workspace_dir,
                "read",
                THOROUGH_RECIPIENT,
                "--team",
                THOROUGH_TEAM,
                "--all",
                "--message-id",
                str(copied_send["message_id"]),
                "--json",
            )
        )
        copied_log_snapshot = parse_json_output(
            run_atm(
                root,
                copied_env,
                copied_fixture.workspace_dir,
                "log",
                "snapshot",
                "--json",
            )
        )
        copied_runtime_ready = (
            copied_doctor.get("summary", {}).get("status") == "healthy"
            and copied_doctor.get("runtime_status", {}).get("readiness") == "ready"
        )
        copied_lane_ok = (
            copied_runtime_ready
            and copied_list.get("count", 0) >= 1
            and copied_send.get("outcome") == "sent"
            and copied_read.get("selected_message_id") == str(copied_send["message_id"])
        )
        if copied_lane_ok:
            pass_row(
                rows["Z1-008"],
                "disposable copied-state doctor/list/send/read all succeeded without touching live host ATM state",
            )
        else:
            fail_row(
                rows["Z1-008"],
                observed=json.dumps(
                    {
                        "doctor": copied_doctor,
                        "list": copied_list,
                        "send": copied_send,
                        "read": copied_read,
                    },
                    indent=2,
                ),
                expected="copied-state doctor, list, send, and read all succeed on a disposable durable baseline clone",
                root_cause="the copied-state lane diverged from the accepted Z.16 disposable baseline contract",
                artifact="copied-state doctor/list/send/read outputs",
                notes="copied-state durable baseline lane failed",
            )
            status = "failed"

        warnings = copied_send.get("warnings", [])
        degraded_warning_blob = json.dumps(warnings).lower()
        degraded_warning_ok = (
            "compatibility append degraded" in degraded_warning_blob
            and "post-send-hook fallback remains available for notification degradation"
            in degraded_warning_blob
        )
        if copied_send.get("outcome") == "sent" and degraded_warning_ok:
            pass_row(
                rows["Z1-006"],
                "copied-state durable send succeeded and surfaced the compatibility append degraded warning after the legacy-array inbox projection failed",
            )
        else:
            fail_row(
                rows["Z1-006"],
                observed=json.dumps(
                    {
                        "send": copied_send,
                        "legacy_inbox_path": str(legacy_inbox_path),
                    },
                    indent=2,
                ),
                expected="durable send succeeds and returns the compatibility append degraded warning when the legacy inbox projection path fails after persistence",
                root_cause="the copied-state compatibility append failure did not surface the accepted degraded warning contract after persistence succeeded",
                artifact="copied-state send --requires-ack --json",
                notes="degraded notification after durable send was not observable",
            )
            status = "failed"

        retry_outcomes = {
            "initial_miss",
            "retry_attempt",
            "acquired",
            "spawn_requested",
            "publish_wait_started",
            "publish_wait_continuing",
            "connected",
        }
        observed_retry_outcomes = set()
        for record in copied_log_snapshot.get("records", []):
            if not isinstance(record, dict):
                continue
            message = str(record.get("message", ""))
            marker = "with outcome "
            if marker in message:
                observed_retry_outcomes.add(message.split(marker, 1)[1].strip())
        if retry_outcomes.issubset(observed_retry_outcomes):
            pass_row(
                rows["Z1-009"],
                "copied-state log snapshot retained the expected retry-visible daemon lifecycle outcomes while the durable send/read path succeeded",
            )
        else:
            fail_row(
                rows["Z1-009"],
                observed=json.dumps(
                    {
                        "observed_outcomes": sorted(observed_retry_outcomes),
                        "records": copied_log_snapshot.get("records", []),
                    },
                    indent=2,
                ),
                expected="log snapshot includes initial_miss, retry_attempt, acquired, spawn_requested, publish_wait_started, publish_wait_continuing, and connected while the durable copied-state lane succeeds",
                root_cause="retry-visible daemon lifecycle evidence was not preserved in the retained copied-state log snapshot",
                artifact="copied-state log snapshot --json",
                notes="retry-visible daemon/runtime evidence was incomplete",
            )
            status = "failed"
    except Exception as exc:
        status = "failed"
        first_pending = next((row for row in rows.values() if row.verdict == "SKIP"), None)
        if first_pending is not None:
            fail_row(
                first_pending,
                observed=str(exc),
                expected="the active smoke step succeeds",
                root_cause="runner-level failure interrupted the thorough smoke lane",
                artifact="runner exception",
                notes="runner aborted during thorough smoke execution",
            )
    finally:
        if daemon_pid is not None and process_is_alive(int(daemon_pid)):
            stop_daemon(int(daemon_pid))
        if copied_daemon_pid is not None and process_is_alive(int(copied_daemon_pid)):
            stop_daemon(int(copied_daemon_pid))
        if status == "passed":
            shutil.rmtree(fixture.root, ignore_errors=True)
            if 'copied_fixture' in locals():
                shutil.rmtree(copied_fixture.root, ignore_errors=True)

    ordered_rows = [rows[row_id].to_payload() for row_id, _ in ROW_MAP["thorough"]]
    summary = {"pass": 0, "fail": 0, "skip": 0}
    for row in ordered_rows:
        summary[row["verdict"].lower()] += 1
    return {
        "level": "thorough",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "binary_sha": binary_sha,
        "duration_secs": round(time.perf_counter() - started, 3),
        "status": status,
        "rows": ordered_rows,
        "summary": summary,
    }


def build_payload(level: str, status: str | None, binary_sha: str) -> dict[str, object]:
    if level == "fast":
        return run_clean_room_lane(
            level="fast",
            binary_sha=binary_sha,
            team=FAST_TEAM,
            operator=FAST_OPERATOR,
            recipient=FAST_RECIPIENT,
            include_validation_check=False,
        )
    if level == "normal":
        return run_clean_room_lane(
            level="normal",
            binary_sha=binary_sha,
            team=NORMAL_TEAM,
            operator=NORMAL_OPERATOR,
            recipient=NORMAL_RECIPIENT,
            include_validation_check=True,
        )
    if level == "thorough":
        return run_thorough(binary_sha)
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

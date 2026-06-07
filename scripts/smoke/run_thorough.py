#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable
import json
import subprocess
import shutil
import time

from run_thorough_graft import enable_graft_config
from run_thorough_graft import run_graft_lane
from run_thorough_retry import verify_retry_evidence
from run_thorough_shared_host import run_shared_host_lane


@dataclass(frozen=True)
class ThoroughSmokeRuntime:
    row_map: dict[str, list[tuple[str, str]]]
    smoke_row_cls: type
    root: Path
    create_clean_room_fixture: Callable[..., Any]
    create_shared_host_fixture_pair: Callable[..., Any]
    clone_fixture: Callable[..., Any]
    smoke_env: Callable[..., dict[str, str]]
    build_release_binaries: Callable[[Path], None]
    parse_json_output: Callable[[Any], dict[str, object]]
    run_atm: Callable[..., Any]
    pass_row: Callable[[Any, str], None]
    fail_row: Callable[..., None]
    failure_mentions: Callable[[Any, str], bool]
    analyze_log_text: Callable[..., Any]
    stop_daemon: Callable[..., None]
    process_is_alive: Callable[[int], bool]
    team: str
    operator: str
    recipient: str
    allowed_error_codes: list[str] = field(default_factory=list)


def run_thorough(binary_sha: str, runtime: ThoroughSmokeRuntime) -> dict[str, object]:
    started = time.perf_counter()
    rows = {
        row_id: runtime.smoke_row_cls(id=row_id, flow=flow)
        for row_id, flow in runtime.row_map["thorough"]
    }
    fixture = runtime.create_clean_room_fixture(
        prefix="z21t.",
        team_name=runtime.team,
        operator=runtime.operator,
        recipient=runtime.recipient,
    )
    enable_graft_config(fixture.workspace_dir)
    base_env = runtime.smoke_env(fixture, identity=runtime.operator, root=runtime.root)
    log_path = fixture.log_dir / "atm.log.jsonl"
    status = "passed"
    daemon_pid: int | None = None
    copied_daemon_pid: int | None = None
    shared_daemon_pid: int | None = None
    copied_fixture = None
    shared_host_fixture_pair = None

    try:
        runtime.build_release_binaries(runtime.root)
        subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "-p",
                "atm-graft",
                "--example",
                "smoke_same_host",
            ],
            cwd=runtime.root,
            check=True,
        )
        runtime.pass_row(rows["Z1-001"], "release smoke binaries built successfully")

        doctor_payload = runtime.parse_json_output(
            runtime.run_atm(runtime.root, base_env, fixture.workspace_dir, "doctor", "--json")
        )
        runtime_status = doctor_payload.get("runtime_status") or {}
        daemon_pid = runtime_status.get("singleton_owner_pid")  # type: ignore[assignment]
        if (
            doctor_payload.get("summary", {}).get("status") == "healthy"
            and runtime_status.get("readiness") == "ready"
        ):
            runtime.pass_row(
                rows["Z1-002"],
                "doctor auto-started the daemon and reported healthy readiness on the clean-room baseline",
            )
        else:
            runtime.fail_row(
                rows["Z1-002"],
                observed=json.dumps(doctor_payload, indent=2),
                expected="doctor summary healthy and runtime_status.readiness=ready",
                root_cause="daemon bootstrap or readiness projection did not reach the accepted healthy baseline",
                artifact="doctor --json",
                notes="clean-room daemon/runtime bring-up did not close cleanly",
            )
            status = "failed"

        runtime.run_atm(
            runtime.root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            runtime.team,
            runtime.operator,
            "--json",
        )
        runtime.run_atm(
            runtime.root,
            base_env,
            fixture.workspace_dir,
            "teams",
            "add-member",
            runtime.team,
            runtime.recipient,
            "--json",
        )
        teams_payload = runtime.parse_json_output(
            runtime.run_atm(runtime.root, base_env, fixture.workspace_dir, "teams", "--json")
        )
        members_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "members",
                "--team",
                runtime.team,
                "--json",
            )
        )
        backup_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "teams",
                "backup",
                runtime.team,
                "--json",
            )
        )
        restore_plan_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "teams",
                "restore",
                runtime.team,
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
            runtime.team in team_names
            and member_names == [runtime.operator, runtime.recipient]
            and Path(str(backup_payload["backup_path"])).exists()
            and restore_plan_payload.get("team") == runtime.team
        )
        if roster_ok:
            runtime.pass_row(
                rows["Z1-003"],
                "teams, members, backup, and restore dry-run all succeeded on the clean-room retained/admin baseline",
            )
        else:
            runtime.fail_row(
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

        copied_fixture = runtime.clone_fixture(
            fixture,
            prefix="z21c.",
            clear_logs=True,
        )

        list_payload = runtime.parse_json_output(
            runtime.run_atm(runtime.root, base_env, fixture.workspace_dir, "list", "--json")
        )
        read_empty_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root, base_env, fixture.workspace_dir, "read", "--all", "--json"
            )
        )
        clear_payload = runtime.parse_json_output(
            runtime.run_atm(runtime.root, base_env, fixture.workspace_dir, "clear", "--json")
        )
        empty_log_snapshot = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root, base_env, fixture.workspace_dir, "log", "snapshot", "--json"
            )
        )
        help_overview = runtime.parse_json_output(
            runtime.run_atm(runtime.root, base_env, fixture.workspace_dir, "help", "--json")
        )
        help_send = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root, base_env, fixture.workspace_dir, "help", "send", "--json"
            )
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
            runtime.pass_row(
                rows["Z1-004"],
                "list/read/clear/log snapshot plus ATM help overview/send guidance all succeeded on the clean-room baseline",
            )
        else:
            runtime.fail_row(
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

        copied_fixture = runtime.clone_fixture(
            fixture,
            prefix="z21c.",
            clear_logs=True,
        )

        send_no_ack_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "send",
                runtime.recipient,
                "thorough smoke no ack",
                "--json",
            )
        )
        send_ack_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "send",
                runtime.recipient,
                "thorough smoke requires ack",
                "--requires-ack",
                "--json",
            )
        )
        ack_required_message_id = str(send_ack_payload["message_id"])
        pending_ack_list_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "list",
                runtime.recipient,
                "--team",
                runtime.team,
                "--pending-ack",
                "--json",
            )
        )
        read_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "read",
                runtime.recipient,
                "--team",
                runtime.team,
                "--all",
                "--message-id",
                ack_required_message_id,
                "--json",
            )
        )
        ack_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "ack",
                ack_required_message_id,
                "thorough smoke ack reply",
                "--team",
                runtime.team,
                "--as",
                runtime.recipient,
                "--json",
            )
        )
        post_ack_list_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "list",
                runtime.recipient,
                "--team",
                runtime.team,
                "--all",
                "--json",
            )
        )
        clear_after_ack_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "clear",
                runtime.recipient,
                "--team",
                runtime.team,
                "--json",
            )
        )
        post_clear_read_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                base_env,
                fixture.workspace_dir,
                "read",
                runtime.recipient,
                "--team",
                runtime.team,
                "--all",
                "--json",
            )
        )
        post_activity_log_snapshot = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root, base_env, fixture.workspace_dir, "log", "snapshot", "--json"
            )
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
            runtime.pass_row(
                rows["Z1-005"],
                "both send modes, pending-ack inspection, recipient read/ack, and post-ack clear/re-read all succeeded on the clean-room baseline",
            )
        else:
            runtime.fail_row(
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

        if not run_graft_lane(runtime, rows, fixture, base_env):
            status = "failed"

        if daemon_pid is not None:
            runtime.stop_daemon(
                int(daemon_pid),
                fixture.home_dir / ".atm" / "daemon" / "atm-daemon.sock",
            )
            daemon_pid = None

        clean_room_log_text = log_path.read_text(encoding="utf-8") if log_path.exists() else ""
        analysis_result = runtime.analyze_log_text(
            clean_room_log_text,
            [
                '"action":"send"',
                '"action":"read"',
                '"action":"ack"',
                '"outcome":"delivery_policy.new_message.primary_nudge"',
                '"outcome":"delivery_policy.ack_reply.delivered"',
                '"action":"shutdown_completed"',
            ],
            allowed_error_codes=runtime.allowed_error_codes,
        )
        analysis = {
            "passed": analysis_result.passed,
            "expected_events": analysis_result.expected_events,
            "missing_events": analysis_result.missing_events,
            "warning_records": analysis_result.warning_records,
            "error_records": analysis_result.error_records,
        }
        if analysis_result.missing_events:
            runtime.fail_row(
                rows["FAST-LOG-001"],
                observed=json.dumps(analysis, indent=2),
                expected="retained log contains send/read/ack/shutdown events plus delivery_policy.new_message.primary_nudge and delivery_policy.ack_reply.delivered",
                root_cause="required debug/verbose healthy-path events were not emitted into the retained log before the negative-path inventory began",
                artifact=str(log_path),
                notes="required fast retained-log events were missing",
            )
            status = "failed"
        else:
            runtime.pass_row(
                rows["FAST-LOG-001"],
                "retained log captured send/read/ack/shutdown plus nudge and ack-reply delivery-policy events before negative-path execution",
            )
        if analysis_result.warning_records or analysis_result.error_records:
            runtime.fail_row(
                rows["FAST-LOG-002"],
                observed=json.dumps(analysis, indent=2),
                expected="retained log contains no warning or error records on the healthy-path portion of the thorough run",
                root_cause="one or more healthy-path events are still being emitted at warn/error severity before negative-path coverage begins",
                artifact=str(log_path),
                notes="retained log severity gate failed",
            )
            status = "failed"
        else:
            runtime.pass_row(
                rows["FAST-LOG-002"],
                "retained log contained no warning or error records during the healthy-path portion of the thorough run",
            )

        error_matrix = [
            (
                "send_invalid_target",
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
                    runtime.smoke_env(fixture, identity=runtime.recipient, root=runtime.root),
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
                runtime.run_atm(
                    runtime.root,
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
            if result.returncode == 0 or not runtime.failure_mentions(result, needle)
        ]
        if error_matrix_failures:
            runtime.fail_row(
                rows["Z1-007"],
                observed=json.dumps(error_matrix_failures, indent=2),
                expected="each CLI validation path fails closed with an actionable typed error instead of mutating state silently",
                root_cause="one or more command-entry validation surfaces did not reject the common error path with the expected recovery guidance",
                artifact="negative-path CLI stderr/stdout matrix",
                notes="thorough CLI validation and recovery guidance matrix failed",
            )
            status = "failed"
        else:
            runtime.pass_row(
                rows["Z1-007"],
                "send/read/ack/list/clear/log/doctor/teams/members/help common error paths all failed closed with explicit actionable guidance",
            )

        copied_env = runtime.smoke_env(
            copied_fixture, identity=runtime.operator, root=runtime.root
        )
        copied_doctor = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root, copied_env, copied_fixture.workspace_dir, "doctor", "--json"
            )
        )
        copied_daemon_pid = copied_doctor.get("runtime_status", {}).get("singleton_owner_pid")  # type: ignore[assignment]
        copied_list = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                copied_env,
                copied_fixture.workspace_dir,
                "list",
                runtime.recipient,
                "--team",
                runtime.team,
                "--all",
                "--json",
            )
        )
        copied_send_result = runtime.run_atm(
            runtime.root,
            copied_env,
            copied_fixture.workspace_dir,
            "send",
            runtime.recipient,
            "thorough copied-state degraded send",
            "--requires-ack",
            "--json",
        )
        copied_send = runtime.parse_json_output(copied_send_result)
        copied_read = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                copied_env,
                copied_fixture.workspace_dir,
                "read",
                runtime.recipient,
                "--team",
                runtime.team,
                "--all",
                "--message-id",
                str(copied_send["message_id"]),
                "--json",
            )
        )
        copied_log_snapshot = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
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
            and copied_list.get("count", 0) == 0
            and copied_send.get("outcome") == "sent"
            and copied_read.get("selected_message_id") == str(copied_send["message_id"])
        )
        if copied_lane_ok:
            runtime.pass_row(
                rows["Z1-008"],
                "disposable copied-state doctor/list/send/read all succeeded without touching live host ATM state",
            )
        else:
            runtime.fail_row(
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
        degraded_warning_ok = "compatibility append degraded" not in degraded_warning_blob
        if copied_send.get("outcome") == "sent" and degraded_warning_ok:
            runtime.pass_row(
                rows["Z1-006"],
                "copied-state durable send stayed on the primary Claude inbox path without compatibility append degradation",
            )
        else:
            runtime.fail_row(
                rows["Z1-006"],
                observed=json.dumps(
                    {
                        "send": copied_send,
                        "warnings": warnings,
                    },
                    indent=2,
                ),
                expected="durable send succeeds on the current Claude inbox primary path without surfacing a compatibility append degraded warning",
                root_cause="the retained runtime still treated the healthy Claude inbox path as degraded or rebuild-only behavior",
                artifact="copied-state send --requires-ack --json",
                notes="primary Claude inbox durable send still emitted degraded compatibility warnings",
            )
            status = "failed"

        shared_host_ok, shared_host_fixture_pair, shared_daemon_pid = run_shared_host_lane(
            runtime, rows
        )
        if not shared_host_ok:
            status = "failed"

        if shared_daemon_pid is not None and runtime.process_is_alive(shared_daemon_pid):
            runtime.stop_daemon(
                shared_daemon_pid,
                shared_host_fixture_pair.workspace_a.home_dir / ".atm" / "daemon" / "atm-daemon.sock",
            )
            shared_daemon_pid = None

        if not verify_retry_evidence(runtime, rows, copied_log_snapshot):
            status = "failed"
    except Exception as exc:
        status = "failed"
        first_pending = next((row for row in rows.values() if row.verdict == "SKIP"), None)
        if first_pending is not None:
            runtime.fail_row(
                first_pending,
                observed=str(exc),
                expected="the active smoke step succeeds",
                root_cause="runner-level failure interrupted the thorough smoke lane",
                artifact="runner exception",
                notes="runner aborted during thorough smoke execution",
            )
    finally:
        if daemon_pid is not None and runtime.process_is_alive(int(daemon_pid)):
            runtime.stop_daemon(
                int(daemon_pid),
                fixture.home_dir / ".atm" / "daemon" / "atm-daemon.sock",
            )
        if copied_daemon_pid is not None and runtime.process_is_alive(int(copied_daemon_pid)):
            runtime.stop_daemon(
                int(copied_daemon_pid),
                copied_fixture.home_dir / ".atm" / "daemon" / "atm-daemon.sock",
            )
        if (
            shared_host_fixture_pair is not None
            and shared_daemon_pid is not None
            and runtime.process_is_alive(shared_daemon_pid)
        ):
            runtime.stop_daemon(
                shared_daemon_pid,
                shared_host_fixture_pair.workspace_a.home_dir
                / ".atm"
                / "daemon"
                / "atm-daemon.sock",
            )
        if status == "passed":
            shutil.rmtree(fixture.root, ignore_errors=True)
            if copied_fixture is not None:
                shutil.rmtree(copied_fixture.root, ignore_errors=True)
            if shared_host_fixture_pair is not None:
                shutil.rmtree(shared_host_fixture_pair.root, ignore_errors=True)

    ordered_rows = [rows[row_id].to_payload() for row_id, _ in runtime.row_map["thorough"]]
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

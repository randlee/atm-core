#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys

from phase_ad_suite import SuiteRowSpec, run_suite


FAST_ROWS = [
    SuiteRowSpec(
        id="AD11-CMD-SEND-001",
        flow="send command preserves caller-context ownership across environment and explicit override paths",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_request_uses_environment_when_overrides_are_absent",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_request_prefers_cli_overrides_over_environment",
            ],
        ],
        pass_note="send stays bound to the shared caller-context contract across environment and explicit override paths",
    ),
    SuiteRowSpec(
        id="AD11-CMD-READ-001",
        flow="read command preserves caller-context ownership across environment and explicit override paths",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_query_uses_environment_when_overrides_are_absent",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_query_prefers_cli_overrides_over_environment",
            ],
        ],
        pass_note="read stays bound to the shared caller-context contract across environment and explicit override paths",
    ),
    SuiteRowSpec(
        id="AD11-CMD-MEMBERS-001",
        flow="members command remains daemon-independent while preserving explicit team override handling",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::members::tests::run_lists_member_roster_without_daemon",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::members::tests::build_query_preserves_team_override",
                "--",
                "--exact",
            ],
        ],
        pass_note="members remains daemon-independent while preserving retained caller-team semantics",
    ),
    SuiteRowSpec(
        id="AD11-CMD-TEAMS-001",
        flow="teams list command remains daemon-independent on the retained CLI surface",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::teams_run_lists_discovered_teams_without_daemon",
                "--",
                "--exact",
            ],
        ],
        pass_note="teams list remains daemon-independent on the retained CLI surface",
    ),
    SuiteRowSpec(
        id="AD11-CMD-LOG-001",
        flow="log command remains daemon-independent with caller-context enforcement at the CLI boundary",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::log::tests::run_snapshot_reads_real_retained_log_without_daemon",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::log::tests::run_snapshot_fails_without_caller_context",
                "--",
                "--exact",
            ],
        ],
        pass_note="log remains daemon-independent and still fails locally when caller context is unavailable",
    ),
    SuiteRowSpec(
        id="AD11-CMD-DOCTOR-001",
        flow="doctor remains identity-free while preserving optional team scoping and the direct local path",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::doctor::tests::build_query_preserves_team_override",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::doctor::tests::execute_runs_direct_local_doctor_path",
                "--",
                "--exact",
            ],
        ],
        pass_note="doctor remains identity-free while preserving optional team scoping and the direct local path",
    ),
    SuiteRowSpec(
        id="AD11-POSTSEND-LOCAL-TMUX-001",
        flow="local tmux post-send requires and uses authoritative pane metadata",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::local_tmux_post_send_emitter_requires_authoritative_pane_id",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::local_tmux_post_send_emitter_uses_authoritative_pane_id",
                "--",
                "--exact",
            ],
        ],
        pass_note="local tmux post-send remains bound to authoritative roster pane metadata",
    ),
    SuiteRowSpec(
        id="AD11-POSTSEND-WARNING-001",
        flow="sender-visible warning fallback survives failed post-send emission",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::local_tmux_post_send_emitter_uses_authoritative_pane_id",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::tests::send_append_failure_routes_to_post_send_hook_fallback",
                "--",
                "--exact",
            ],
        ],
        pass_note="failed post-send emission still degrades into a sender-visible warning after durable send success",
    ),
]

NORMAL_ROWS = FAST_ROWS + [
    SuiteRowSpec(
        id="AD11-CMD-ACK-001",
        flow="ack command preserves caller-context ownership across environment and explicit override paths",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::ack::tests::build_request_uses_environment_when_overrides_are_absent",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::ack::tests::build_request_prefers_cli_overrides_over_environment",
                "--",
                "--exact",
            ],
        ],
        pass_note="ack stays bound to the shared caller-context contract across environment and explicit override paths",
    ),
    SuiteRowSpec(
        id="AD11-CMD-LIST-001",
        flow="list command preserves retained filters while keeping caller-context ownership explicit",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::list::tests::build_query_preserves_limit_and_filters",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::list::tests::build_query_uses_environment_when_overrides_are_absent",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::list::tests::build_query_prefers_cli_overrides_over_environment",
                "--",
                "--exact",
            ],
        ],
        pass_note="list preserves retained filters while staying bound to explicit caller-context ownership",
    ),
    SuiteRowSpec(
        id="AD11-CMD-CLEAR-001",
        flow="clear command preserves caller-context ownership across environment and explicit override paths",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::clear::tests::build_query_uses_environment_when_overrides_are_absent",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::clear::tests::build_query_prefers_cli_overrides_over_environment",
                "--",
                "--exact",
            ],
        ],
        pass_note="clear stays bound to the shared caller-context contract across environment and explicit override paths",
    ),
    SuiteRowSpec(
        id="AD11-CMD-TEAMS-ADD-MEMBER-001",
        flow="teams add-member preserves the retained home-dir payload contract",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::add_member_build_request_preserves_atm_and_member_home_dirs",
                "--",
                "--exact",
            ],
        ],
        pass_note="teams add-member preserves the retained home-dir payload contract",
    ),
    SuiteRowSpec(
        id="AD11-CMD-TEAMS-UPDATE-MEMBER-001",
        flow="teams update-member preserves caller context and fails locally when mandatory caller context is missing",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::update_member_build_request_preserves_target_and_caller_context",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::update_member_requires_identity_from_environment",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::update_member_requires_team_from_environment_not_positional_target",
                "--",
                "--exact",
            ],
        ],
        pass_note="teams update-member preserves caller context and still fails locally when mandatory caller context is missing",
    ),
    SuiteRowSpec(
        id="AD11-CMD-TEAMS-BACKUP-001",
        flow="teams backup preserves retained team scoping and remains daemon-independent in dry-run execution",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::backup_build_request_preserves_team",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::backup_and_restore_dry_run_execute_without_daemon",
                "--",
                "--exact",
            ],
        ],
        pass_note="teams backup preserves retained team scoping and remains daemon-independent in dry-run execution",
    ),
    SuiteRowSpec(
        id="AD11-CMD-TEAMS-RESTORE-001",
        flow="teams restore preserves retained path and dry-run behavior without requiring the daemon",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::restore_build_request_preserves_from_path_and_dry_run",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::backup_and_restore_dry_run_execute_without_daemon",
                "--",
                "--exact",
            ],
        ],
        pass_note="teams restore preserves retained path and dry-run behavior without requiring the daemon",
    ),
    SuiteRowSpec(
        id="AD11-ROSTER-REPAIR-001",
        flow="fixture evidence preserves repaired pane metadata through team-admin and doctor projections",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "team_admin::tests::update_member_repairs_existing_roster_metadata_and_projects_config",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "doctor::tests::run_doctor_reports_pane_and_home_dir_drift_from_roster_truth",
                "--",
                "--exact",
            ],
        ],
        pass_note="fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths",
    ),
    SuiteRowSpec(
        id="AD17-ULID-001",
        flow="retained ATM message identity stays ULID-only on the accepted line",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "schema::inbox_message::tests::atm_message_id_parses_from_ulid_string",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "workflow::tests::workflow_key_uses_atm_message_id",
                "--",
                "--exact",
            ],
        ],
        pass_note="ULID-only message identity remains enforced in retained schema and workflow state",
    ),
    SuiteRowSpec(
        id="AD17-READ-001",
        flow="read mutation and contains filtering stay self-consistent on the durable store-backed path",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "--test",
                "mailbox_locking",
                "read_store_backed_display_mutation_ignores_mailbox_file_lock",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "read::tests::actionable_selection_preserves_parent_context_for_add_details",
                "--",
                "--exact",
            ],
        ],
        pass_note="read mutation still reports the post-mutation state and contains filtering still sees the durable full-body projection",
    ),
    SuiteRowSpec(
        id="AD17-CI-001",
        flow="windows CI retains the explicit atm-daemon lane on the accepted line",
        commands=[
            [
                "rg",
                "-n",
                "Run atm-daemon tests",
                ".github/workflows/ci.yml",
            ],
            [
                "python3",
                "-c",
                "from pathlib import Path; data = Path('.github/workflows/ci.yml').read_text(encoding='utf-8'); raise SystemExit(1 if \"if: runner.os != 'Windows'\" in data else 0)",
            ],
        ],
        pass_note="the explicit atm-daemon CI lane remains present and the Windows skip guard is absent",
    ),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Phase AD smoke runner")
    parser.add_argument("level", choices=("fast", "normal", "thorough"))
    parser.add_argument("--write-artifacts", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.level == "thorough":
        command = [sys.executable, "scripts/smoke/run_thorough.py"]
        if args.write_artifacts:
            command.append("--write-artifacts")
        completed = subprocess.run(command, check=False)
        return completed.returncode
    specs = FAST_ROWS if args.level == "fast" else NORMAL_ROWS
    payload = run_suite(args.level, specs, write_artifacts=args.write_artifacts)
    return 0 if payload["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())

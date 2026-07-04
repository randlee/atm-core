#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys

from phase_ad_suite import SuiteRowSpec, run_suite


FAST_ROWS = [
    SuiteRowSpec(
        id="AD11-SEND-ENV-001",
        flow="send command uses environment caller context when overrides are absent",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_request_uses_environment_when_overrides_are_absent",
            ],
        ],
        pass_note="send caller-context resolution stays bound to environment when explicit overrides are absent",
    ),
    SuiteRowSpec(
        id="AD11-READ-ENV-001",
        flow="read command uses environment caller context when overrides are absent",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_query_uses_environment_when_overrides_are_absent",
            ],
        ],
        pass_note="read caller-context resolution stays bound to environment when explicit overrides are absent",
    ),
    SuiteRowSpec(
        id="AD11-MEMBERS-ENV-001",
        flow="members command remains daemon-independent under environment-only caller context",
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
        ],
        pass_note="members remains daemon-independent while using the shared caller-context resolver",
    ),
    SuiteRowSpec(
        id="AD11-TEAMS-ENV-001",
        flow="teams command remains daemon-independent under environment-only caller context",
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
        pass_note="teams remains daemon-independent while using the shared caller-context resolver",
    ),
    SuiteRowSpec(
        id="AD11-LOG-ENV-001",
        flow="log command remains daemon-independent under environment-only caller context",
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
        ],
        pass_note="log remains daemon-independent while using the shared caller-context resolver",
    ),
    SuiteRowSpec(
        id="AD11-DOCTOR-TEAM-001",
        flow="doctor preserves optional team override without caller identity",
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
        ],
        pass_note="doctor preserves optional team scoping without caller identity",
    ),
    SuiteRowSpec(
        id="AD11-DOCTOR-DIRECT-001",
        flow="doctor still executes the direct local path without caller identity",
        commands=[
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
        pass_note="doctor still executes the direct local path without caller identity",
    ),
    SuiteRowSpec(
        id="AD11-POSTSEND-PANE-001",
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
        id="AD11-POSTSEND-WARN-001",
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
        id="AD11-SEND-OVERRIDE-001",
        flow="send command prefers explicit CLI caller-context overrides over environment values",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_request_prefers_cli_overrides_over_environment",
            ],
        ],
        pass_note="send remains bound to explicit CLI caller context when provided",
    ),
    SuiteRowSpec(
        id="AD11-READ-OVERRIDE-001",
        flow="read command prefers explicit CLI caller-context overrides over environment values",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_query_prefers_cli_overrides_over_environment",
            ],
        ],
        pass_note="read remains bound to explicit CLI caller context when provided",
    ),
    SuiteRowSpec(
        id="AD11-MEMBERS-OVERRIDE-001",
        flow="members command preserves explicit team override",
        commands=[
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
        pass_note="members preserves explicit CLI team override instead of ambient environment values",
    ),
    SuiteRowSpec(
        id="AD11-UPDATE-MEMBER-IDENTITY-LOCAL-001",
        flow="update-member fails locally when caller identity is unavailable",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::update_member_requires_identity_from_environment",
                "--",
                "--exact",
            ],
        ],
        pass_note="update-member rejects missing caller identity locally before any retained execution",
    ),
    SuiteRowSpec(
        id="AD11-UPDATE-MEMBER-TEAM-LOCAL-001",
        flow="update-member fails locally when caller team is unavailable",
        commands=[
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
        pass_note="update-member rejects missing caller team locally before any retained execution",
    ),
    SuiteRowSpec(
        id="AD11-LOG-LOCAL-001",
        flow="log command fails locally when caller context is unavailable",
        commands=[
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
        pass_note="log fails at CLI entry instead of guessing or dispatching into retained execution",
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

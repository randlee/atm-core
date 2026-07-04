#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys

from phase_ad_suite import SuiteRowSpec, run_suite
from run_thorough_graft import graft_commands


THOROUGH_ROWS = [
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
        id="AD11-XREPO-001",
        flow="sender roster home_dir governs post-send config lookup across repos",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::sender_config_root_prefers_home_dir_and_falls_back_to_cwd",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::load_post_send_config_uses_sender_roster_metadata_not_caller_cwd",
                "--",
                "--exact",
            ],
        ],
        pass_note="post-send config discovery remains anchored to sender roster metadata rather than ambient caller cwd, preserving cross-repo local-send behavior",
    ),
    SuiteRowSpec(
        id="AD11-GRAFT-001",
        flow="graft-backed post-send emission path remains optional and explicit",
        commands=graft_commands(),
        pass_note="the graft-backed emission seam delegates through the dedicated graft port and surfaces failure without leaking graft ownership into the core send path",
    ),
    SuiteRowSpec(
        id="AD11-AUTH-001",
        flow="update-member auth checks and infallible add-member projection are closed",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "update_member_rejects_caller_",
            ],
            [
                "rg",
                "-n",
                r"fn build_member_add_roster_record\(request: &AddMemberRequest\) -> RosterEntry",
                "crates/atm-core/src/team_admin.rs",
            ],
            [
                "rg",
                "-n",
                r"validate_update_member_caller\(",
                "crates/atm-core/src/team_admin.rs",
            ],
        ],
        pass_note="the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail",
    ),
    SuiteRowSpec(
        id="AD11-READINESS-001",
        flow="phase-ad readiness and boundary artifacts fail closed",
        commands=[
            [sys.executable, "scripts/validate_release.py", "phase-ad-readiness"],
        ],
        pass_note="Phase AD readiness records, smoke artifacts, and PostSendHookEmitter boundary inventory are all present and wired into the retained validation gate",
    ),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Phase AD thorough smoke runner")
    parser.add_argument("--write-artifacts", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    payload = run_suite("thorough", THOROUGH_ROWS, write_artifacts=args.write_artifacts)
    return 0 if payload["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys

from phase_ad_suite import SuiteRowSpec, run_suite
from run_thorough_graft import graft_commands


THOROUGH_ROWS = [
    SuiteRowSpec(
        id="AD11-ENV-001",
        flow="env-only caller context for retained command surfaces",
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
                "build_query_uses_environment_when_overrides_are_absent",
            ],
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
                "commands::teams::tests::teams_run_lists_discovered_teams_without_daemon",
                "--",
                "--exact",
            ],
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
        pass_note="env-only caller context succeeds for retained CLI surfaces that require it, and daemon-independent retained command paths stay operational",
    ),
    SuiteRowSpec(
        id="AD11-OVERRIDE-001",
        flow="explicit CLI caller-context overrides win when supported",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_request_prefers_cli_overrides_over_environment",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "build_query_prefers_cli_overrides_over_environment",
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
        pass_note="commands with retained override surfaces stay bound to explicit CLI caller context instead of ambient environment values",
    ),
    SuiteRowSpec(
        id="AD11-LOCAL-001",
        flow="caller-context failures stay local before retained execution",
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
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::teams::tests::update_member_requires_team_from_environment_not_positional_target",
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
        pass_note="missing caller identity or caller team still fails at CLI entry instead of guessing or dispatching into retained execution",
    ),
    SuiteRowSpec(
        id="AD11-DOCTOR-001",
        flow="doctor remains identity-free and optional-team scoped",
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
        pass_note="doctor still executes without caller identity or caller team while preserving explicit team scoping",
    ),
    SuiteRowSpec(
        id="AD11-POSTSEND-001",
        flow="local tmux post-send and sender-visible warning fallback",
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
        pass_note="local tmux nudges still use authoritative pane metadata and forced emission failure still degrades into sender-visible warning behavior",
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

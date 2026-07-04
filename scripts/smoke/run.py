#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys

from phase_ad_suite import SuiteRowSpec, run_suite


FAST_ROWS = [
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
]

NORMAL_ROWS = FAST_ROWS + [
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

#!/usr/bin/env python3
from __future__ import annotations

import argparse
import subprocess
import sys

from phase_ad_suite import SuiteRowSpec, run_suite


POST_SEND_ROWS = [
    SuiteRowSpec(
        id="AD29-POSTSEND-EXTERNAL-001",
        flow="external post-send hook success suppresses built-in fallback while preserving durable send success",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::external_post_send_hook_takes_precedence_over_built_in_nudge",
                "--",
                "--exact",
            ],
        ],
        pass_note="external post-send hook success keeps the built-in nudge path inactive while durable send success remains intact",
    ),
    SuiteRowSpec(
        id="AD29-POSTSEND-PARTIAL-001",
        flow="mixed post-send hook outcomes preserve durable delivery while surfacing sender-visible warnings",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::mixed_success_hook_accounting_preserves_delivery_and_warning",
                "--",
                "--exact",
            ],
        ],
        pass_note="mixed hook accounting preserves durable delivery success and retains a sender-visible warning for failed matches",
    ),
    SuiteRowSpec(
        id="AD29-POSTSEND-BUILTIN-001",
        flow="built-in fallback covers both tmux and graft recipients when no external hook matches",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::built_in_fallback_dispatches_local_tmux_through_emitter",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::hook::tests::graft_fallback_dispatches_through_emitter_without_tmux_fields",
                "--",
                "--exact",
            ],
        ],
        pass_note="built-in fallback stays honest for both tmux-backed and graft-backed recipients when no external hook matches",
    ),
    SuiteRowSpec(
        id="AD29-POSTSEND-RESET-001",
        flow="deleting a prior override row restores the built-in default template path",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "atm-storage-rusqlite",
                "nudge_template_override_store::tests::sqlite_override_store_returns_none_after_override_row_is_deleted",
                "--",
                "--exact",
            ],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail-core",
                "send::nudge_template::tests::resolve_template_body_uses_default_when_no_override_exists",
                "--",
                "--exact",
            ],
        ],
        pass_note="removing a stored override row re-exposes the built-in default template instead of leaving an implicit disabled state behind",
    ),
    SuiteRowSpec(
        id="AD29-POSTSEND-DISABLE-001",
        flow="explicitly disabled built-in template state skips local post-send delivery cleanly",
        commands=[
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "commands::internal_nudge::tests::internal_nudge_run_skips_delivery_when_template_is_explicitly_disabled",
                "--",
                "--exact",
            ],
        ],
        pass_note="the explicit disabled-template state becomes a documented no-delivery path instead of an accidental empty-string side effect",
    ),
]

FAST_ROWS = POST_SEND_ROWS
NORMAL_ROWS = POST_SEND_ROWS


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

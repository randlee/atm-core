#!/usr/bin/env python3
"""Enforce the ADR-054 nudge-taxonomy vocabulary boundary.

Two checks, mirroring `scripts/check-legacy-mailbox-paths.py`:

1. `FORBIDDEN_PATTERNS` rejects retired pre-ADR-054 identifiers with no
   compatibility alias (`PostSendHookEmitter`) and the pre-rename built-in
   target variant ADR-054/D4 renames to `PostSendBuiltInTarget::LocalSteer`
   (`PostSendBuiltInTarget::LocalTmux`). A small, explicit
   `ALLOWED_FORBIDDEN_LITERALS` table exempts the historical doc-comment that
   names the retired identifier on purpose.
2. `ALLOWED_NUDGE_IDENTIFIERS` is the frozen inventory of every
   `nudge`-family identifier accepted at Sprint AQ1 cut time, generated via:

       rg -o '[A-Za-z_]*[Nn]udge[A-Za-z_]*' crates | sort -u

   plus the Sprint AQ1 trait-foundation identifiers pre-seeded ahead of their
   owning lane (L1 `atm-storage`, L2 `atm-core`/`atm-graft`) landing, so the
   gate stays green as those lanes commit. Any `nudge`-family identifier
   introduced outside this inventory must be kind-qualified (`Steer`/`Queue`)
   per ADR-054 (a); growing the inventory requires an ADR-054 amendment or a
   follow-on ADR (see ADR-054 appendix, deferred rename inventory).
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import re
import sys


@dataclass(frozen=True)
class Violation:
    path: Path
    line_number: int
    label: str
    line: str


@dataclass(frozen=True)
class AllowedLiteral:
    path_suffix: str
    line_pattern: re.Pattern[str]


FORBIDDEN_PATTERNS = (
    (
        "retired post-send hook emitter name (no compatibility alias; use MessageReceivedHookEmitter)",
        re.compile(r"\bPostSendHookEmitter\b"),
    ),
    (
        "pre-ADR-054 built-in target variant (renamed to PostSendBuiltInTarget::LocalSteer)",
        re.compile(r"\bPostSendBuiltInTarget::LocalTmux\b"),
    ),
)

ALLOWED_FORBIDDEN_LITERALS = (
    AllowedLiteral(
        "crates/atm-core/src/boundary/mod.rs",
        re.compile(r"deliberately has no compatibility alias"),
    ),
)

# Identifiers matching the nudge-family pattern below that were already
# accepted in the tree at Sprint AQ1 cut time, plus AQ1 trait-foundation
# identifiers pre-seeded ahead of their owning lane's commit (crate-placement
# decision D1; see docs/plans/phase-aq/aq1-blueprint.md). Entries that never
# match `NUDGE_IDENTIFIER_PATTERN` (for example `MemberKey`, `mark_pending`,
# the `PendingNudgeStore` non-nudge method names, and the SQLite index name)
# are included anyway so a future broadening of the scan pattern does not
# need a second pass over the AQ1 contract surface.
ALLOWED_NUDGE_IDENTIFIERS = frozenset(
    {
        "_enqueue_nudge", "_nudge", "_on_nudge", "acquire_host_nudge_helper_permit",
        "bounded_host_nudge_injector_caps_helper_growth_under_repeated_hangs",
        "bounded_host_nudge_injector_timeout_does_not_wedge_future_delivery",
        "BoundedHostNudgeInjector", "built_in_nudge_template_kind_from_post_send_event",
        "BuiltInNudgeSinkTarget", "BuiltInNudgeTemplateKind", "claim_next_pending",
        "clear_nudge_template_command",
        "clear_nudge_template_executes_through_shared_override_boundary",
        "clear_nudge_template_override_deletes_row_and_reports_state",
        "clear_nudge_template_override_with_store", "clear_pending_on_handoff",
        "clear_pending_on_read", "ClearNudgeTemplate", "ClearNudgeTemplateCommand",
        "ClearNudgeTemplateOverrideOutcome", "ClearNudgeTemplateOverrideRequest",
        "disable_nudge_template_command",
        "disable_nudge_template_executes_through_shared_override_boundary",
        "disable_nudge_template_override_saves_disabled_row_through_boundary",
        "disable_nudge_template_override_with_store", "DisableNudgeTemplate",
        "DisableNudgeTemplateCommand", "DisableNudgeTemplateOverrideOutcome",
        "DisableNudgeTemplateOverrideRequest", "DummyNudgeTemplateOverrideStore",
        "DummyPendingNudgeStore", "empty_nudge_template_body", "EmptyNudgeTemplateBody",
        "ensure_mail_message_states_nudge_columns", "ensure_team_nudge_template_override_columns",
        "ensure_team_nudge_template_override_columns_migrates_legacy_empty_rows_to_disabled",
        "error_nudge", "expected_nudge_substring", "FakeNudge", "first_nudge", "from_host_nudge",
        "graft_nudge_sink_delivers_to_host_injector",
        "graft_nudge_sink_injects_rendered_xml_and_full_message_body",
        "graft_nudge_sink_returns_typed_error_envelope", "GraftNudgeSink", "GraftNudgeTarget",
        "host_nudge", "HostNudge", "HostNudgeInjector", "idx_mail_message_states_pending",
        "idx_team_nudge_template_overrides_team_name", "inject_nudge", "injected_nudge",
        "internal_nudge", "internal_nudge_input_accepts_explicit_disabled_template_state",
        "internal_nudge_input_reads_resolved_envelope",
        "internal_nudge_run_skips_delivery_when_template_is_explicitly_disabled", "InternalNudge",
        "InternalNudgeCommand", "InternalNudgeEnvelope", "InternalNudgeInput", "list_pending_members",
        "listen_for_graft_nudges", "load_nudge_template_override", "local_nudge",
        "LocalTmuxNudgeTarget", "mark_pending", "MAX_NUDGE_ATTEMPTS", "MemberKey", "nnudge",
        "NoopNudgeTemplateOverrideStore", "nudge", "Nudge", "nudge_attempts", "nudge_count",
        "nudge_delivered", "nudge_dispatch", "nudge_message_id", "nudge_mode", "nudge_pending_at",
        "nudge_preserves_typed_source_chat_id", "nudge_sink", "nudge_template",
        "nudge_template_override_store", "nudge_timeout_secs", "NudgeClaim", "NudgeKind", "NudgeMode",
        "nudges", "NudgeTemplateOverrideStore", "on_nudge", "pending_nudge_store", "PendingNudgeStore",
        "primary_nudge", "print_clear_nudge_template_override_result",
        "print_disable_nudge_template_override_result", "print_set_nudge_template_override_result",
        "PyNudge", "python_nudge_constructor_validates_immutable_event_fields", "PythonNudgeInjector",
        "qualified_nudge_sender_identity", "queue_marker_set", "rebuild_received_hook_dispatch",
        "receive_host_nudge_result", "receiver_callback_receives_the_canonical_typed_nudge",
        "receiver_loop_delivers_direct_nudge_and_returns_ack_under_repeated_load",
        "RecordingNudgeTemplateOverrideStore", "release_pending", "render_built_in_nudge",
        "render_built_in_nudge_for_dispatch", "render_built_in_nudge_populates_placeholders",
        "render_resolved_built_in_nudge", "rendered_nudge", "request_nudge", "requeue_pending",
        "ResolvedBuiltInNudgeTemplate", "RunErrorNudge", "RunPrimaryNudge",
        "set_nudge_template_build_request_rejects_empty_template_body_before_core",
        "set_nudge_template_build_request_rejects_invalid_kind_before_core",
        "set_nudge_template_command", "set_nudge_template_executes_through_shared_override_boundary",
        "set_nudge_template_override_rejects_caller_team_mismatch",
        "set_nudge_template_override_rejects_empty_body",
        "set_nudge_template_override_rejects_invalid_kind",
        "set_nudge_template_override_saves_row_through_boundary",
        "set_nudge_template_override_with_store", "SetNudgeTemplate", "SetNudgeTemplateCommand",
        "SetNudgeTemplateOverrideOutcome", "SetNudgeTemplateOverrideRequest",
        "spawn_host_nudge_helper", "SqliteNudgeTemplateOverrideStore", "SqlitePendingNudgeStore",
        "storage_and_nudge_router", "StorageAndNudgeRouter", "team_nudge_template_overrides",
        "TeamNudgeTemplateOverrideMode", "TeamNudgeTemplateOverrideRow", "TmuxNudgeSink",
        "validate_nudge_template_body", "validate_nudge_template_override_team",
        "warn_host_nudge_result", "with_default_nudge_template_override_store", "with_nudge_mode",
        "with_pending_nudge_store",
    }
)

# Same shape as the `rg` inventory-generation command in the module docstring.
NUDGE_IDENTIFIER_PATTERN = re.compile(r"[A-Za-z_]*[Nn]udge[A-Za-z_]*")


def iter_rust_sources(repo_root: Path) -> tuple[Path, ...]:
    crates_dir = repo_root / "crates"
    return tuple(sorted(crates_dir.rglob("*.rs")))


def is_allowed_forbidden_literal(relative_path: str, line: str) -> bool:
    return any(
        relative_path.endswith(allowed.path_suffix) and allowed.line_pattern.search(line)
        for allowed in ALLOWED_FORBIDDEN_LITERALS
    )


def find_violations(repo_root: Path) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    for path in iter_rust_sources(repo_root):
        relative_path = path.relative_to(repo_root).as_posix()
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            for label, pattern in FORBIDDEN_PATTERNS:
                if pattern.search(line) and not is_allowed_forbidden_literal(relative_path, line):
                    violations.append(Violation(Path(relative_path), line_number, label, line.strip()))
            for match in NUDGE_IDENTIFIER_PATTERN.findall(line):
                if match not in ALLOWED_NUDGE_IDENTIFIERS:
                    violations.append(
                        Violation(
                            Path(relative_path),
                            line_number,
                            f"new nudge-family identifier outside the ADR-054 frozen inventory: {match!r}",
                            line.strip(),
                        )
                    )
    return tuple(violations)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Enforce the ADR-054 nudge-taxonomy vocabulary gate on workspace Rust sources."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="Workspace root to scan. Defaults to the repository that owns this script.",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    repo_root = args.repo_root.resolve()
    violations = find_violations(repo_root)
    if violations:
        print("nudge-taxonomy failed")
        for violation in violations:
            print(
                f"{violation.path}:{violation.line_number}: {violation.label}: {violation.line}"
            )
        return 1

    print("nudge-taxonomy passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

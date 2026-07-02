# Phase S.5 — Guardrails And Bounded Queue Queries

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.5"
status: in-review
estimated_scope: M
```

## Goal

Close the remaining Phase S process and product-surface gaps by:

- making the no-flaky-test contract phase-wide
- defining the mechanical lint families that must prevent hang-prone
  regression patterns from re-entering the same-host daemon line
- documenting the bounded mailbox-query split where `atm list` owns metadata
  search and `atm read` owns single-message detail fetch
- assigning every remaining post-S.4 implementation item to an execution-ready
  follow-on sprint

## Governing Requirements

- `REQ-P-TEST-001`
- `REQ-P-LINT-POSTMORTEM-001`
- `REQ-P-LIST-001`
- `REQ-P-READ-001`
- `REQ-CORE-TEST-RUNTIME-001`
- `REQ-CORE-LIST-001`
- `REQ-CORE-COMPAT-001`
- `REQ-DAEMON-TEST-004`
- `REQ-DAEMON-PLATFORM-002`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`
- `docs/adr/ADR-009-bounded-queue-query-surface.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Hard Dependencies

- `integrate/phase-S` contains the merged S.1 through S.4 implementation line
- the existing fixed-sleep, singleton, portability, and runtime-waits lint
  gates remain green before S.5 follow-on planning starts

## Required Work

1. Tighten the Phase S wording from "no fixed sleeps" to the stronger
   no-flaky-test and no-unbounded-wait policy.
1.1 Update the active Phase S plan and sprint docs so same-host daemon tests
    explicitly forbid:
   - unbounded waits
   - missed-wakeup-sensitive waits with no bounded fallback
   - panic-unsafe global/shared test hooks
   - retry-until-success loops with no state predicate
1.2 Carry the same contract into:
   - `docs/testing-guidelines.md`
   - `docs/cross-platform-guidelines.md`
   - top-level requirements and architecture docs

2. Define the mechanical anti-flake guardrail inventory.
2.1 Record which rules are feasible now in the default `just lint` path:
   - fixed-sleep test hygiene, with the current repository-local rule treated
     as the proving implementation for later `sc-lint` extraction
   - daemon-spawn helper rejection
   - production bare `Condvar::wait(...)`
   - production discarded `wait_timeout*` results
   - targeted same-host daemon test checks for cheap unbounded-wait syntax
2.2 Record which rules require deferred analyzer or rule-design work:
   - path-sensitive `JoinHandle::join()` safety
   - polling-loop terminate-state placement
   - panic-safe cleanup proof for global test hooks
   - bounded-wait result handling in test code

3. Name the intended lint ownership for each family.
3.1 Reusable Rust-aware analyzer rules remain `sc-lint-*` candidates.
3.2 ATM-specific repository policy checks remain `.just/` or `scripts/` lints
    until their rule shape is proven reusable.

4. Update the Phase S issue inventory so the remaining policy and guardrail
   gaps are explicit rather than implied by QA findings.

5. Record any ADR needed for the repository-wide no-flaky-test decision and
   enforcement partition.

6. Harden the Phase S triage process so canonical `.triage/<phase>/findings/*.ttl`
   records are committed to git at the correct workflow point.
6.1 Set the commit point at the team-lead triage batch stage:
   - after all parallel `qa-triage` agents finish writing `.ttl` files
   - after aggregation confirms the batch is complete
   - before any branch-scoped fix dispatch to `arch-ctm`
   - on the phase integration-branch worktree, which is the canonical triage
     source of truth for that phase
6.2 Update the relevant triage prompt/skill so the process explicitly forbids
    leaving phase findings untracked in the working tree until phase end and
    explicitly requires `triage_root` to live under the integration-branch
    worktree rather than an arbitrary feature branch.
6.3 Keep this track separate from the mailbox-read planning follow-up; the
    triage git-commit gap is an independent process-hardening item.
6.4 Land any `qa-triage` prompt edits from a develop-based worktree if that
    prompt is shared outside the active Phase S planning branch.

7. Add the mailbox-query reliability and CLI-surface track.
7.1 Record that GitHub issues `#213` and `#214` are not isolated read-path
    bugs; they expose a broader queue-inspection design problem where default
    reads still materialize too much mailbox history.
7.2 Document the accepted command split:
   - `atm list` is the bounded metadata-search surface
   - `atm read` returns one selected full message
7.3 Define the accepted list-row field contract:
   - `message_id`
   - `summary`
   - `from`
   - `timestamp`
   - `read`
   - `pending_ack`
   - `task_id` when present
7.4 Define the shared list/read filter contract:
   - optional target inbox
   - `--team`
   - `--from`
   - `--since`
   - `--task`
   - `--contains`
   - `--unread`
   - `--pending-ack`
   - `--all`
7.4.1 Define the legacy `atm read` flag migration:
   - `--unread-only` -> deprecated alias for `--unread`
   - `--pending-ack-only` -> deprecated alias for `--pending-ack`
   - `--history` -> deprecated alias for `--all`
   - `--since-last-seen` remains an explicit restatement of the default
     watermark behavior
7.5 Define `atm read` selection behavior:
   - bare `atm read` returns the most recent unread actionable message
   - pending-ack messages are prioritized ahead of non-ack unread messages
   - selector-driven reads return the most recent match when multiple messages
     match
   - successor/update chains are one logical message and selection operates on
     their terminal node
   - `--task <task-id>` chooses the most recent terminal-node logical message
     among task-linked matches
   - the read result must expose `match_count` and
     `additional_match_count`
7.6 Record the bounded-query rule:
   - default queue inspection must be bounded by query behavior, not merely by
     render truncation
   - durable SQLite rows must not tolerate malformed JSON as a normal degraded
     read mode
7.7 Define the Claude Code compatibility-envelope rule:
   - ATM-authored JSONL body export defaults to a `128 KiB` cap
   - config `[atm].claude_jsonl_body_export_max_bytes` may lower that cap,
     including `0` for stub-only ATM-authored export
   - if ATM skips the full body, the JSONL `text` field becomes exactly
     `atm read --message-id <id>`
   - summary text remains populated when ATM exports that retrieval stub
   - full ATM-authored bodies remain durable in SQLite
   - Claude-native inbound messages are never rewritten into ATM retrieval
     stubs
   - watcher/reconcile logic must treat ATM-authored compatibility projection
     updates as idempotent and must not create self-induced churn loops

8. Create the follow-on execution sprint line for all remaining Phase S work.
8.1 `S.6` must own the daemon/runtime post-mortem remediation items:
   - `RSH-001` `crates/atm-daemon/src/composition.rs::shutdown_background_lanes`
   - `RSH-014` `crates/atm-daemon/src/lifecycle_control.rs` Unix EOF wake
     propagation gap
   - `WIN-001` Windows graceful shutdown regression in the daemon shutdown
     signal path and its test coverage
   - `ATM-QA-S4-001`
     `crates/atm-daemon/src/local_ipc_transport.rs::prepare_local_ipc_endpoint`
8.2 `S.7` must own the bounded queue-query implementation:
   - add `atm list`
   - convert `atm read` to single-message logical-current selection
   - implement shared list/read filters and legacy read-flag migration
   - implement bounded metadata-query paths instead of full-surface read
     materialization
8.3 `S.8` must own the ATM-authored Claude JSONL compatibility-envelope
    implementation:
   - `[atm].claude_jsonl_body_export_max_bytes`
   - oversized ATM-authored body stub export
   - watcher/reconcile no-churn handling for ATM-authored compatibility
     projections
8.4 `FTQ-001` remains explicitly deferred from the execution line here. Keep it
    recorded as a lint/analyzer follow-up item rather than an S.6-S.8 code
    implementation item unless a later audit reclassifies it.

## Required Document Updates

- `docs/plans/phase-S/plan-phase-S.md`
- `docs/plans/phase-S/issues.md`
- `docs/plans/phase-S/sprint-S0.md`
- `docs/plans/phase-S/sprint-S1.md`
- `docs/plans/phase-S/sprint-S2.md`
- `docs/plans/phase-S/sprint-S3.md`
- `docs/plans/phase-S/sprint-S4.md`
- `docs/plans/phase-S/sprint-S6.md`
- `docs/plans/phase-S/sprint-S7.md`
- `docs/plans/phase-S/sprint-S8.md`
- `docs/testing-guidelines.md`
- `docs/cross-platform-guidelines.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`
- `docs/adr/ADR-009-bounded-queue-query-surface.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`
- `docs/adr/INDEX.md`
- `docs/read-behavior.md`
- `docs/claude-code-message-schema.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/commands/list.md`
- `docs/atm/commands/read.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/modules/config.md`
- `docs/atm-core/modules/list.md`
- `docs/atm-core/modules/read.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `.claude/agents/qa-triage.md`
- `.claude/skills/triaging-findings/SKILL.md`
- `boundaries/atm-core/config-ingress.toml`
- `boundaries/atm-core/inbox-export.toml`
- `boundaries/atm-daemon/daemon-inbox-export.toml`

## Acceptance Criteria

- Phase S has one explicit phase-wide no-flaky-test contract rather than a
  narrow fixed-sleep-only rule
- the active docs state that a test which might hang is invalid even if it
  does not use `thread::sleep(...)`
- the active docs distinguish feasible-now lint families from deferred
  analyzer work
- the default development gate remains `just lint`, and the S.5 docs name
  which new anti-flake families belong there
- the mechanical guardrail plan names the intended implementation home for
  each rule family (`sc-lint-*` vs `.just/` / `scripts/`)
- the triage workflow names an explicit git-commit step for phase `.ttl`
  records before dev dispatch begins
- the active planning docs state that the canonical `triage_root` for a phase
  lives on that phase's integration-branch worktree
- the chosen triage commit point prevents a repeat of the Phase S gap where
  findings remained untracked until manual intervention
- the active product and crate-local docs describe one clean queue-inspection
  split where `atm list` is the search/index surface and `atm read` is the
  single-message detail surface
- the docs state that default queue inspection must stay bounded even as
  SQLite-backed mailbox history grows without a fixed upper bound
- the docs define how `atm read` reports multiple matches without returning
  multiple full message bodies
- the docs define how successor/task-linked logical messages collapse before
  `atm read` chooses one current match
- the docs define the legacy `atm read` flag migration instead of leaving the
  deprecation surface implicit
- the docs define the ATM-authored Claude JSONL export cap, retrieval-stub
  behavior, and watcher no-churn rule
- the docs state that stubbed ATM-authored JSONL exports retain summary text
  and never rewrite Claude-native inbound messages
- S.6, S.7, and S.8 sprint docs exist and assign every remaining post-S.4
  implementation item to an execution-ready sprint
- `RSH-001`, `RSH-014`, `WIN-001`, and `ATM-QA-S4-001` have explicit sprint
  ownership with concrete code targets
- `FTQ-001` is explicitly recorded as a deferred lint/analyzer item rather
  than being left ambiguous between planning and implementation

## Required Validation

- `just lint`

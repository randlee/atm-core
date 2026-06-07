# AA.12 Malformed Claude Inbox Recovery Hardening

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.12
worktree: ../atm-core-worktrees/feature/pAA-s12-malformed-claude-inbox-recovery
branch: feature/pAA-s12-malformed-claude-inbox-recovery
status: planned
estimated_scope: medium
```

## Goal

Make the Claude inbox read path fail-soft: preserve every recoverable message,
emit explicit degraded warnings for malformed fragments, and avoid dropping an
entire sprint conversation because one producer wrote bad JSON.

## Scope Summary

This sprint is the malformed-ingress recovery line. It does not widen the
legal schema contract beyond what `AA.8` and `AA.10` freeze. Instead, it
defines how ATM reads the inbox when the file or one record is malformed,
truncated, or mixed good/bad. The required behavior is best-effort salvage with
traceable degraded output, not an all-or-nothing parser failure, unless the
root document is genuinely unreadable end-to-end.

## Governing Sources

- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/src/service_runtime.rs`
- `tools/schema_models/test_schema_models.py`

## Prerequisites

- `AA.9`
- `AA.10`

## Out Of Scope

- no expansion of the legal Claude inbox schema
- no reapproval of historical ATM-owned fields beyond the derivative support
  explicitly frozen in `AA.10`
- no SQLite durable-schema work

## Deliverables

- The repo has one explicit malformed-ingress policy for Claude inbox reads:
  - legal current-schema records parse normally
  - legal derivative-schema records tolerated by `AA.10` still parse normally
  - malformed-but-localized records degrade to a readable sentinel/warning
    result without hiding unrelated valid messages in the same inbox
  - only root-level unreadable content that cannot be segmented or salvaged may
    fail the whole read with a structured error

- The read-path contract is spelled out with a code-shaped outcome surface, or
  an equivalent accepted API, so implementation choice is not left open:

  ```rust
  enum InboxReadItem {
      Message(ClaudeCodeInboxMessage),
      Degraded {
          summary: String,
          warning: String,
          raw_fragment: Option<String>,
      },
  }
  ```

- The docs and tests define which corruption classes are expected to salvage:
  - malformed ATM metadata on an otherwise valid message
  - one malformed message object adjacent to valid message objects
  - truncated tail after at least one valid earlier message
  - unknown additive metadata on otherwise valid Claude messages

- The docs and tests define which corruption classes remain terminal:
  - unreadable root content with no segmentable message objects
  - invalid root shape that cannot be interpreted as any supported inbox
    collection surface

- An ADR is added or updated if the chosen fail-soft policy changes the
  repository-wide shared-inbox boundary contract rather than only
  implementation details. If no existing ADR can absorb that decision, create
  `docs/adr/ADR-017-claude-inbox-fail-soft-read-policy.md` and register it in
  `docs/adr/INDEX.md`.

## Split Recommendation

Keep malformed-ingress recovery separate from schema-contract support removal.
`AA.10` decides what is legal on read; `AA.12` decides how aggressively ATM
salvages around malformed input that is outside that legal contract.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA12.md` exists with the planned branch/worktree
- one malformed shared-inbox fragment cannot hide unrelated valid messages when
  those valid messages are still segmentable
- malformed ATM-owned metadata on an otherwise valid Claude message does not
  drop that message
- malformed-salvageable reads return a degraded/error variant, do not panic,
  and do not corrupt the underlying inbox file
- the read path emits explicit degraded warnings/sentinels for salvageable bad
  fragments instead of a generic opaque parse failure
- any remaining terminal-failure cases are enumerated in docs and backed by
  tests
- if the malformed-recovery policy is architectural, a new or updated ADR is
  linked from the sprint outputs

## Required Validation

- `python3 -m unittest tools.schema_models.test_schema_models`
- `cargo test -p agent-team-mail-core`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA12.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/adr/ADR-017-claude-inbox-fail-soft-read-policy.md` if a new ADR is
  required
- `docs/adr/INDEX.md`

## Risks And Watchouts

- if this sprint silently widens the legal schema instead of only hardening the
  recovery path, `AA.10` will be undermined
- if “best effort” is left undefined, one implementation may salvage messages
  while another still aborts the whole inbox
- if degraded warnings are not explicit and testable, operators will not know
  whether a message was missing, malformed, or fully consumed

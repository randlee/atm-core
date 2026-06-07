# AA.10 Remove Historical ATM JSON Compatibility From 1.2

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.10
worktree: ../atm-core-worktrees/feature/pAA-s10-remove-historical-atm-json
branch: feature/pAA-s10-remove-historical-atm-json
status: planned
estimated_scope: medium
```

## Goal

Remove support promises for historical ATM-owned inbox JSON schema variants so
ATM 1.2 supports only the current legal Claude Code inbox schema on the
Claude-to-Claude shared inbox path.

## Scope Summary

This sprint is the JSON compatibility removal line. It deletes the repo-owned
contract that still promises read support for historical ATM top-level inbox
fields and `metadata.atm` machine-state shapes, and it replaces that promise
with explicit 1.2 behavior for obsolete inputs: ignore, reject, or repair
through a named admin path, but do not treat them as supported live schema.

## Governing Sources

- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/legacy-atm-message-schema.md`
- `tools/schema_models/atm_message_schema.py`
- `tools/schema_models/legacy_atm_message_schema.py`
- `tools/schema_models/test_schema_models.py`

## Prerequisites

- `AA.8`
- `AA.9`

## Out Of Scope

- no SQLite durable-schema removal
- no change to Claude-owned message semantics
- no silent extension of the supported 1.2 schema surface

## Deliverables

- The repo no longer presents historical ATM JSON variants as supported active
  shared inbox schema for 1.2.

- The removal set is explicit and authoritative:
  - historical top-level ATM fields such as `message_id`, `source_team`,
    `pendingAckAt`, `acknowledgedAt`, and `acknowledgesMessageId`
  - historical alert-only top-level fields such as `atmAlertKind` and
    `missingConfigPath`
  - `metadata.atm.*` as an active shared inbox contract

- The docs, schema models, and tests agree on 1.2 behavior for obsolete JSON:
  - current Claude schema is supported
  - obsolete ATM-authored JSON is not treated as a supported live contract
  - any remaining repair or import path is explicitly named and does not leak
    into normal send/read/runtime behavior

- The repo no longer ships a “legacy ATM message schema” doc/model as if that
  were part of the accepted 1.2 shared inbox contract.

## Split Recommendation

Keep this sprint focused on JSON-schema support removal. SQLite historical
schema removal is its own risk surface and belongs in `AA.11`.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA10.md` exists with the planned branch/worktree
- `docs/legacy-atm-message-schema.md` is removed or clearly retired from the
  active 1.2 contract with no remaining source-of-truth references that imply
  live support
- `tools/schema_models/legacy_atm_message_schema.py` is removed or retired from
  the active test gate
- no requirements/architecture/Phase AA doc still promises normal read-path
  support for historical ATM top-level JSON or `metadata.atm.*`
- the active tests validate the current Claude contract and the chosen fail
  behavior for obsolete ATM-authored JSON

## Required Validation

- `python3 -m unittest tools.schema_models.test_schema_models`
- `cargo test -p agent-team-mail-core`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA10.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Risks And Watchouts

- if this sprint leaves documentation and tests half-migrated, QA will read one
  contract while runtime enforces another
- if obsolete ATM-authored JSON remains silently accepted without an explicit
  policy, operators will not know whether they are on supported or unsupported
  1.2 behavior

# AA.10 Remove Historical ATM JSON Compatibility From 1.2

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.10
worktree: ../atm-core-worktrees/feature/pAA-s10-remove-historical-atm-json
branch: feature/pAA-s10-remove-historical-atm-json
status: complete
estimated_scope: medium
```

## Goal

Stop treating historical ATM-authored inbox JSON extensions as the primary or
forward-write contract for 1.2 while preserving read compatibility for legal
Claude-schema derivatives produced by ATM 1.1, the historical ATM producer
that wrote Claude-envelope messages plus additive ATM-owned fields such as
`metadata.atm.*`.

## Scope Summary

This sprint is the JSON contract-tightening line. It removes the repo-owned
claim that historical ATM top-level inbox fields and `metadata.atm`
machine-state shapes are the active primary shared-inbox contract for 1.2, but
it does not make legal Claude-schema derivatives invalid on read. Historical
ATM-authored additive fields remain read-compatible when they extend the legal
Claude envelope; ATM ignores or strips the ATM-owned machine metadata rather
than failing schema validation for those derivative shapes. Malformed-record
salvage policy is not part of this sprint and is planned separately in
`AA.12`.

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
- no malformed-record salvage/recovery behavior changes

## Deliverables

- The repo no longer presents historical ATM JSON variants as the supported
  active primary shared inbox schema for 1.2.

- ATM 1.1 messages that extend the Claude Code schema by adding metadata fields
  such as `metadata.atm.*` MUST parse successfully. The read path silently
  ignores unknown or ATM-owned additive fields. Removing live-contract status
  means no new code depends on those fields; it does not mean reading them
  fails.

- The contract split is explicit and authoritative:
  - current Claude Code-native inbox schema is the primary shared inbox
    contract
  - historical ATM additive fields such as `message_id`, `source_team`,
    `pendingAckAt`, `acknowledgedAt`, and `acknowledgesMessageId` are
    read-compatible derivative fields, not the forward-write contract
  - historical alert-only top-level fields such as `atmAlertKind` and
    `missingConfigPath` remain read-compatible only as additive derivatives
  - `metadata.atm.*` is not the active primary contract, but inbox records that
    carry it still validate as legal additive derivatives and are ignored or
    stripped by the read path rather than rejected as invalid schema

- The docs, schema models, and tests agree on 1.2 behavior for derivative JSON:
  - current Claude schema is supported
  - legal ATM-authored schema derivatives do not fail validation solely because
    they contain additive ATM metadata
  - those derivatives are not described as the primary or forward-write
    contract
  - retirement behavior is explicit for truly obsolete/non-derivative inputs:
    they are either ignored safely, rejected with a structured error, or
    routed to a named repair/import-only admin path, and none of those paths
    leak into normal send/read/runtime behavior

- The repo no longer ships a “legacy ATM message schema” doc/model as if that
  were the accepted primary 1.2 shared inbox contract.

## Split Recommendation

Keep this sprint focused on JSON-schema support removal. SQLite historical
schema removal is its own risk surface and belongs in `AA.11`.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA10.md` exists with the planned branch/worktree
- `docs/phase-AA/readiness.md` is updated consistently with the accepted AA.10
  closure state, including the retained rule that legal ATM 1.1 additive
  derivatives still parse successfully on read
- `docs/legacy-atm-message-schema.md` is retained only as a read-compatibility
  contract for legal additive derivatives or is clearly retired in favor of an
  equivalent read-compatibility contract with no wording that implies
  primary/write ownership
- `tools/schema_models/legacy_atm_message_schema.py` and/or the active schema
  test gate continue validating legal additive derivative shapes on read
- no requirements/architecture/Phase AA doc still promises that historical ATM
  top-level JSON or `metadata.atm.*` are the primary or forward-write 1.2
  contract
- schema-derivative messages (ATM 1.1 format = Claude Code schema + metadata
  extension fields) parse without error; tests explicitly assert non-failure
  for these inputs
- the active tests validate:
  - the current Claude contract
  - top-level ATM additive derivative shapes
  - `metadata.atm.*` additive derivative shapes
  - derivative-schema validation does not fail solely because of tolerated ATM
    additive metadata

## Required Validation

- `python3 -m unittest tools.schema_models.test_schema_models`
- `cargo test -p agent-team-mail-core`
- `git diff --check`
- tests include at least one schema-derivative fixture (ATM 1.1 payload with
  Claude schema fields plus `metadata.atm` additions) that asserts parse
  success and field-ignore behavior

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

- if this sprint confuses “not the primary contract” with “invalid on read,”
  it will break legal ATM 1.1 Claude-schema derivatives that must remain
  accepted
- if documentation and tests are half-migrated, QA will read one contract while
  runtime enforces another

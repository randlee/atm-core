# AA.8 Claude Code Inbox Schema Contract Alignment

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.8
worktree: ../atm-core-worktrees/feature/pAA-s8-claude-schema-contract
branch: feature/pAA-s8-claude-schema-contract
status: complete
estimated_scope: medium
```

## Goal

Freeze the current Claude Code inbox JSON contract as the primary shared inbox
surface and align ATM’s docs, schema models, and fixture-backed tests to that
contract.

## Scope Summary

This sprint is schema-contract and documentation alignment only. It does not
change the runtime append implementation yet. The work is to prove exactly what
the legal Claude inbox message schema is, compare that contract to real
`team-lead -> quality-mgr` traffic, and remove wording that incorrectly treats
the current Claude inbox file shape as legacy.

## Governing Sources

- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `tools/schema_models/claude_code_message_schema.py`
- `tools/schema_models/test_schema_models.py`

## Prerequisites

- `AA.7`

## Out Of Scope

- no runtime append-path behavior changes
- no SQLite schema removal
- no continued promise of historical ATM 1.1 JSON support beyond what this
  sprint explicitly freezes as current contract or hands to `AA.10`

## Deliverables

- The current Claude Code inbox envelope is frozen in the docs and model as the
  primary shared inbox contract:

  ```json
  {
    "from": "team-lead",
    "text": "message body or encoded task payload",
    "timestamp": "2026-06-06T23:23:32.085Z",
    "read": true,
    "summary": "optional summary",
    "color": "optional Claude-owned producer field"
  }
  ```

- Real historical `team-lead -> quality-mgr` message samples are redacted into
  fixture coverage, and the fixtures are validated against the current Pydantic
  model.

- The docs explicitly distinguish three categories:
  1. current Claude Code-native envelope
  2. tolerated unknown additive fields on that envelope
  3. historical ATM-owned additive fields that are not part of the current
     Claude contract and are queued for `AA.10`

- Any sentence that classifies the current JSON-array Claude inbox file shape
  itself as legacy is corrected or removed.

- The schema enforcement tests prove the current contract against at least:
  - native Claude envelope
  - current real-world `team-lead -> quality-mgr` samples
  - any still-documented ATM additive shape that remains temporarily accepted

## Split Recommendation

Keep this sprint limited to contract discovery, wording correction, and
fixture-backed model alignment. Runtime behavior changes belong in `AA.9`.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA8.md` exists with the planned branch/worktree
- `docs/phase-AA/readiness.md` is updated consistently with the accepted AA.8
  closure state
- `docs/claude-code-message-schema.md` defines the current shared inbox
  contract clearly enough that `req-qa` can enumerate the legal envelope
  directly from the sprint outputs
- `tools/schema_models/claude_code_message_schema.py` matches the current
  contract
- real `team-lead -> quality-mgr` fixture samples validate against the model
- no Phase AA or schema-ownership doc still calls the current JSON-array
  Claude inbox file shape “legacy”
- any remaining historical ATM JSON support is called out explicitly as
  follow-on removal scope for `AA.10`, not as the current Claude contract

## Required Validation

- `python3 -m unittest tools.schema_models.test_schema_models`
- `git diff --check`
- `python3 - <<'PY'` contract-term scan proving the banned stale phrases are absent from `docs/`, `tools/`, `crates/`, and `scripts/`

## Required Document Updates

- `docs/phase-AA/sprint-AA8.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`

## Risks And Watchouts

- if this sprint stops at prose and does not add real fixture-backed validation,
  the schema can drift again without anyone noticing
- if the sprint silently treats historical ATM additive fields as current
  Claude contract, `AA.10` will start from a false baseline

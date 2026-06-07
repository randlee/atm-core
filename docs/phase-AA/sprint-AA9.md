# AA.9 Current Claude Inbox Primary-Path Repair

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.9
worktree: ../atm-core-worktrees/feature/pAA-s9-claude-inbox-primary-path
branch: feature/pAA-s9-claude-inbox-primary-path
status: complete
estimated_scope: medium
```

## Goal

Make the current Claude Code inbox JSON file shape the working primary ATM
compatibility path instead of treating it as a degraded or legacy fallback.

## Scope Summary

This sprint is runtime-behavior repair. It closes the gap where ATM’s retained
append path treats any inbox file that begins with `[` as an unsupported
rebuild-only mailbox, even though the current Claude Code inbox files are
JSON-array mailboxes. The deliverable is a normal append path that works on the current
Claude mailbox shape and reserves repair/rebuild only for malformed or truly
unsupported inbox state.

## Governing Sources

- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `scripts/smoke/run.py`
- `reports/smoke/smoke-thorough.md`

## Prerequisites

- `AA.8`

## Out Of Scope

- no removal of historical ATM JSON compatibility promises
- no SQLite schema-removal work
- no broad resend/replay redesign unrelated to the Claude inbox primary path

## Deliverables

- The normal retained append path recognizes the current Claude inbox file
  shape as supported primary behavior instead of degraded-only behavior.

- The current guard in `service_runtime.rs` is replaced with one that
  distinguishes:
  - current supported Claude JSON-array inbox JSON
  - current supported JSONL append surface, if retained intentionally
  - malformed or unsupported mailbox content that really must fail closed

- The repair/rebuild seam remains explicit, but it is no longer the expected
  path for a healthy current Claude inbox file and is reserved for malformed or
  unsupported mailbox state.

- Smoke/report wording is corrected so a healthy send to a current Claude inbox
  does not claim success only after a rebuild-only projection warning.

- The retained runtime tests and smoke coverage prove the supported path using
  the current Claude mailbox shape.

## Split Recommendation

Do not mix runtime primary-path repair with historical-schema deletion. Once
the current path is fixed, historical JSON removal belongs in `AA.10`.

## Acceptance Criteria

- `docs/phase-AA/sprint-AA9.md` exists with the planned branch/worktree
- `docs/phase-AA/readiness.md` is updated consistently with the accepted AA.9
  closure state
- `compat_inbox_uses_legacy_array_format(...)` no longer classifies the current
  Claude JSON-array inbox file by its leading `[` alone
- ATM can append or otherwise write through the normal primary path to the
  current Claude inbox file shape without surfacing a degraded rebuild-only
  warning
- the retained repair/rebuild seam is reserved for malformed or explicitly
  unsupported mailbox state, not healthy current Claude inbox files
- smoke/report wording is consistent with the repaired primary path

## Required Validation

- `cargo test -p agent-team-mail-core`
- `python3 scripts/smoke/run.py thorough --write-artifacts`
- `git diff --check`

## Required Document Updates

- `docs/phase-AA/sprint-AA9.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/claude-code-message-schema.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `reports/smoke/smoke-thorough.md` if this sprint uses a checked-in regenerated
  artifact or wording fixture

## Risks And Watchouts

- if the fix remains heuristic and still keys on file prefix rather than legal
  Claude mailbox shape, the same bug will reappear under another name
- if the runtime still treats the Claude path as degraded-only, operators will
  keep seeing false warnings on the primary inbox surface

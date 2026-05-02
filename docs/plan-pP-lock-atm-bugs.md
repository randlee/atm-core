# Phase P Lock + ATM Context Plan

## Goal

Document the two active Phase P blockers so implementation can move directly to
a bounded sprint instead of reopening architecture debate during QA.

Bugs covered:
- lock requirements violation on the current mailbox write path
- ATM-authored message schema breaking Claude context injection

## Bug 1: Lock Requirements Violation

### Root Cause

Phase P improved mailbox locking, stale-sentinel handling, and timeout
behavior, but the current line still encodes mailbox locks as part of the
runtime correctness model. That does not satisfy the stricter operational
requirement that stale/orphaned locks stop being a product-blocking failure.

Current state:
- `read_only` paths can avoid locks
- mutating mailbox paths still depend on lock acquisition
- the system still relies on stale-lock recovery behavior, including the
  5-minute cron sweep, rather than eliminating lock dependence

### Affected Files / Paths

- `crates/atm-core/src/mailbox/lock.rs`
- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `docs/requirements.md`
- `docs/architecture.md`

### Fix Approach

Short term:
- treat this as a release gate, not another hardening sprint
- prove whether `integrate/phase-P` is good enough as an interim lock-relief
  release

Gate criteria:
- stale/orphaned lock artifact does not wedge command flow
- write contention fails or recovers in bounded time
- read-only paths still work under contention
- crash/restart style recovery works without relying solely on the cron sweep
- `send -> ack -> clear` remains operable after the fault

If the gate passes:
- Phase P can ship as an interim release only

If the gate fails:
- stop investing in mailbox-lock architecture and move directly to the
  SQLite-backed replacement phase

### Acceptance Criteria

- explicit release-gate checklist exists in `docs/project-plan.md`
- missing deterministic lock tests are added
- `develop` and `integrate/phase-P` are both run against the same gate
- release decision is PASS/FAIL, not another open-ended stabilization loop

## Bug 2: ATM Context Injection Broken

### Root Cause

ATM-authored messages are being written to the shared Claude inbox with
ATM-owned machine fields at the top level. Claude-native teammate delivery
works, and Claude-native plus `metadata.atm` also works, but ATM top-level
fields break the context-injection path.

Proven behavior:
- native Claude envelope works
- native Claude envelope plus `metadata.atm` works
- ATM-authored top-level fields such as `message_id`, `source_team`, and
  `pendingAckAt` are the compatibility break

### Affected Files / Paths

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/schema/inbox_message.rs`
- mailbox compatibility helpers that serialize or project ATM-authored inbox
  records
- `tools/schema_models/claude_code_message_schema.py`
- `tools/schema_models/atm_message_schema.py`
- `tools/schema_models/test_schema_models.py`
- `docs/claude-code-message-schema.md`
- `docs/atm-message-schema.md`

### Fix Approach

Forward write rule:
- shared Claude inbox top level stays Claude-native only
- ATM-owned machine fields move under `metadata.atm`

Required migrations for ATM-authored writes:
- `message_id` -> `metadata.atm.messageId`
- `source_team` -> `metadata.atm.sourceTeam`
- `pendingAckAt` -> `metadata.atm.pendingAckAt`
- `acknowledgedAt` -> `metadata.atm.acknowledgedAt`
- `acknowledgesMessageId` -> `metadata.atm.acknowledgesMessageId`
- `taskId` -> `metadata.atm.taskId`

Compatibility rule:
- legacy top-level ATM fields remain read-compatible during transition
- forward writes must not emit those fields at the top level

Validation rule:
- top-level envelope must validate against
  `ClaudeCodeInboxMessage`
- ATM-authored inbox messages with metadata must validate against
  `AtmMetadataEnvelope`

### Acceptance Criteria

- ATM-authored `send` and `ack` writes produce Claude-native top-level fields
  plus optional `metadata.atm`
- schema-model tests fail if ATM machine fields leak to the top level
- QA reviews the schema fix change set itself
- QA also reviews the final comparison-run evidence; comparison review is
  PASS/FAIL, not authorization for more code churn

## Sprint Recommendation

Open one bounded implementation sprint from `integrate/phase-P`:
- fix the ATM inbox schema bug first
- run the lock release gate without expanding scope
- send the schema change set and the gate evidence to QA

Do not:
- reopen broader mailbox architecture redesign inside this sprint
- use the gate as cover for another general stabilization cycle
- mix the later SQLite SSOT work into this sprint

---
title: AI.37 Hermes graft recovery summary
status: proposed
branch: feature/pAI-s37-hermes-recovery-summary
target: integrate/phase-ai-31-33
depends_on: AI.36
---

# AI.37 — Hermes graft recovery summary

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.37
worktree: feature/pAI-s37-hermes-recovery-summary
branch: feature/pAI-s37-hermes-recovery-summary
status: proposed
estimated_scope: small Rust/Python recovery contract
```

## Goal

After a profile restart, use the durable daemon mailbox to generate one
non-message recovery wake-up summary after exactly ten seconds when work is
available. Graft must not store, replay, read, or acknowledge mail itself.

## Scope Summary

This sprint adds a count-only projection over the existing `ReadOutcome` and
a one-shot Python recovery scheduler. It deliberately stops at an injected
host-neutral recovery callback. AI.38 binds that callback and live nudges to
Hermes's steer API.

## Governing Requirements

- `REQ-GRAFT-NOTIFY-002`
- `REQ-GRAFT-HERMES-003`
- `REQ-CORE-GRAFT-001`
- `REQ-CORE-COMPAT-003` persistence precedes post-send behavior

## Governing ADRs

- ADR-033 — shared daemon client/API contract
- ADR-037 — `ChatId` identity
- ADR-043 — durable recovery is mailbox-derived, not graft-owned

## Governing Boundaries

- `DaemonApiClient` and existing `ReadQuery`/`ReadOutcome` own counts.
- `atm-graft-python` only projects existing daemon results; it does not access
  SQLite or create an HTTP resource.
- The Python bridge owns one-shot scheduling but not host routing policy.

## Prerequisites

- AI.36 receiver ownership is merged and receiver activation has a reliable
  listening transition.

## Hard Dependencies

- AI.38 consumes the host-neutral live/recovery callback introduced here.

## Non-Goals

- No durable graft queue, retries, polling loop, bulk message replay, or
  automatic acknowledgement.
- No additional daemon endpoint, database schema, or message state.
- No configurable delay: the first supported recovery delay is exactly ten
  seconds, hardcoded in the Python adapter.

## Sub-Tasks

### 1. Count-only daemon-client projection

Development work:

1. First commit sets all releasable assemblies to `1.4.0-beta-ai.37`.
2. Reuse `ReadQuery` with its existing non-mutating classified read and
   `ReadOutcome.bucket_counts`; do not add a new mailbox API or query source.
3. Add an explicit graft method and Python projection that exposes only:

```rust
pub struct MailboxWorkCounts {
    pub unread: usize,
    pub pending_ack: usize,
}

fn mailbox_work_counts(&self) -> Result<MailboxWorkCounts, AtmError>;
```

4. `pending_ack` means inbound messages for this profile that require its ATM
   acknowledgement and remain unacknowledged. It is not an outbound delivery
   receipt count.

Required tests:

- returned values equal the existing `ReadOutcome.bucket_counts` for empty,
  unread-only, pending-ack-only, and mixed mailboxes;
- the method neither changes read state nor acknowledgement state;
- Python binding preserves both values as integers without exposing stored
  message bodies.

### 2. One-shot recovery trigger

Development work:

1. Add a bridge-level recovery hook whose only payload is the immutable count
   value, not message content:

```python
@dataclass(frozen=True)
class MailboxRecoveryNotice:
    unread: int
    pending_ack: int

    def render(self) -> str:
        return f"ATM: {self.unread} unread messages; {self.pending_ack} acknowledgements pending."
```

2. After a receiver first reports `listening`, schedule exactly one callback
   with `loop.call_later(10.0, ...)`. At callback time call
   `mailbox_work_counts()` once. Invoke the recovery hook only when either
   count is non-zero.
3. Cancellation on `disconnect()` is mandatory. Reconnect creates one new
   ten-second window only after the prior timer is cancelled/finished.
4. A live nudge during the delay remains a live nudge. It does not cancel,
   accelerate, or multiply the recovery summary.

Required tests with a fake loop/clock (no wall-clock sleep):

- no callback before 10.0 seconds and exactly one at 10.0 seconds;
- mixed counts render the exact concise text; zero/zero invokes nothing;
- disconnect before 10 seconds invokes nothing;
- reconnect after cancellation has one new timer, never two;
- a live-nudge callback plus a later recovery summary are distinct bounded
  notifications, while no individual durable message is replayed.

### 3. Observability and contract documentation

Development work:

1. Record structured recovery scheduled/cancelled/counts/summary-emitted
   events without mail body or capability data.
2. Update the Hermes adapter contract to state that the host must use its
   normal ATM skills after a summary; graft never consumes the mail.

Required tests:

- emitted event names distinguish zero-work from summary-emitted;
- source test rejects `read()`, `acknowledge()`, persistence, or retry-loop
  calls from the recovery scheduler.

## Split Recommendation

Keep actual Hermes steer wiring out of this sprint. This produces a
deterministic and host-neutral recovery contract that can be reviewed without
depending on an external gateway API.

## Acceptance Criteria

1. One successful receiver activation schedules one fixed ten-second recovery
   check.
2. The check uses existing daemon mailbox counts and makes no mutation.
3. Nonzero work emits one concise count summary; empty work emits nothing.
4. Disconnect/reconnect cannot duplicate scheduled summaries.
5. Graft contains no durable mail or conversation state.

## Required Validation

```text
cargo test -p atm-graft -- --nocapture
cargo test -p atm-graft-python -- --nocapture
python3 -m unittest crates/atm-graft-python/tests/test_hermes_bridge.py
just lint
just test
```

## Required Document Updates

- `docs/atm-graft/requirements.md`
- `docs/plans/phase-ai/hermes-graft-adapter-contract.md`
- ADR-043 status/evidence note

## Risks And Watchouts

- Counts must come from the daemon's normal mailbox view at timer execution,
  not activation-time snapshots.
- Do not turn the one-shot timer into a periodic poller or retry mechanism.
- The scheduler must remain testable with an injected clock/loop.

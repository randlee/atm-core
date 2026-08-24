# Sprint AQ3 — Queue: Tmux Idle-Drain

Status: draft · Branch: `feature/aq-3-queue-tmux` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

The tmux received-hook side of queue: maintain the per-recipient pending
FIFO (derived — AQ1) and, when the harness transitions to idle, nudge the
NEXT unread queued message. One nudge per idle transition; a backlog drains
at the harness's own pace.

Verified baseline: harness state arrives as `TeamMemberHeartbeatRequest`
(`HeartbeatActivity: ActiveToolUse | Idle | SessionEnded`,
protocol.rs:330-342) recorded by `RuntimeHealth.record_heartbeat()`
(runtime_health.rs:101-158) into an in-memory map — explicitly
"observability, not durable state", polling-only, no transition events
today. Activity mapping: ActiveToolUse→Active, Idle→Idle,
SessionEnded→Offline.

## Deliverables

1. **Idle-transition hook**: `RuntimeHealth.record_heartbeat()` gains a
   transition notification firing on transitions to `Idle`. Execution
   contract: `on_transition` fires **strictly after the mutex is
   released** — never inside the critical section its module doc scopes to
   in-memory fields — and the storage claim it triggers runs under
   `spawn_blocking`/a bounded blocking pool, never inline on the
   heartbeat-handling task. `RuntimeHealth`'s module doc is updated
   (best-effort sink, non-authoritative; recovery sweep is the correctness
   backstop) and `MemberStateTransitionSink` gets a
   `boundaries/atm-http-runtime/` record (fixed/internal, ADR-001
   sealed-supertrait pattern).

```rust
pub trait MemberStateTransitionSink: Send + Sync {
    fn on_transition(&self, member: &MemberKey,
                     from: RuntimeMemberState, to: RuntimeMemberState);
}
```

2. **Drain step**: on an idle transition for member M, call AQ1's
   `PendingNudgeStore::next_pending` + `claim_pending` — the backend-side
   atomic claim is THE at-most-once mechanism, shared verbatim with the
   recovery sweep; the claim winner fires the ordinary steer delivery for
   exactly that message-id via the existing selector path. **One message
   per transition.** Read rows never appear (markers cleared by AQ1's
   read hook).
3. **Recovery sweep**: low-frequency periodic pass (maintenance-cadence
   precedent) draining for currently-Idle members with non-empty FIFOs —
   covers restarts (in-memory `RuntimeHealth` resets) and missed
   transitions; also re-attempts failed graft handoffs (AQ2) up to ADR-054
   (f)'s max auto-retry count — past it, auto-retry stops and the "stuck"
   health signal surfaces (attempt count tracked per marker). One per
   member per pass. Daemon shutdown cancels and joins the
   task within the daemon deadline.
4. **Observability**: per-drain structured event (`subsystem`/`action`/
   `outcome` + `{member, msg_id}`) and a cumulative drained counter on the
   health report. Recorded exception: events emit via `emit_daemon_event`
   for consistency with the maintenance-worker precedent; no license to
   refactor it.

## Acceptance criteria

1. Active→Idle with two pending unread → exactly the oldest drains (test
   double records it), marker clears; the second drains on the next
   transition.
2. Read-before-drain → skipped entirely; next pending drains instead.
3. Restart with pending rows → recovery sweep drains for an Idle member
   with no transition.
4. Concurrency: transition drain and sweep pass fired concurrently for one
   member with one pending message → exactly one nudge and one clear
   (atomic-claim test).
5. No transition, no tick → no nudge. Shutdown mid-pass cancels and joins
   within the deadline.
6. `just test` + daemon integration suite, all three lanes.

## Paths to delete

None.

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows.
- Live evidence: a real tmux-harness member queued 2 messages while busy,
  observed draining one-per-idle-transition; transcript committed.

## Non-closure / out of scope

- Durable heartbeat history; subscription APIs beyond the internal sink;
  priority ordering; re-nudge/reminder policies.

## Dependencies

- must_follow: AQ1 (kinds, store, suppression) — merge-forward before every
  dev/fix round.
- parallel_safe: AQ2 (tmux drain vs graft channel — disjoint emitters).

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
   `PendingNudgeStore::claim_next_pending` — the backend-side atomic
   select-and-claim is THE at-most-once mechanism, shared verbatim with
   the recovery sweep; the claim winner dispatches for exactly that
   message-id via the existing selector path (steer for tmux members), and
   a failed dispatch is requeued via `requeue_pending`. **One message
   per transition.** Read rows never appear (markers cleared by AQ1's
   read hook).
3. **Recovery sweep** — **kind-agnostic**: a low-frequency periodic pass
   (maintenance-cadence precedent) that claims via
   `PendingNudgeStore::claim_next_pending` and re-dispatches through the
   ordinary `MessageReceivedHookSelector`, which routes by recipient kind
   (graft → AQ2's queue channel, tmux → steer). The sweep contains **no
   graft-specific logic and calls no AQ2 code** — that is why AQ2/AQ3 stay
   parallel-safe — and a failed dispatch is requeued via
   `requeue_pending`, so retry bounds and the stuck flag are enforced by
   AQ1's store, not here. **Channel pre-check (owned HERE; consumes
   AQ2.5's classifier seam)**: before claiming for member M, the sweep
   calls AQ2.5's `DeliveryChannel` classifier (extended with `HerdrSteer`
   by AQ2.6) and claims **only** when M is classified `TmuxSteer` or
   `Graft` — the channels this sweep can dispatch through. Herdr members
   are owned solely by AQ2.7's lifecycle-gated pump; this sweep never
   claims them. Bare-CLI members never have sweep work by
   construction (AQ2.5's emitter clears their pending marker on FIFO
   append — handoff semantics), and the pre-check is the belt to that
   suspender: no claim, no dispatch failure, no attempt increment for
   any member the classifier does not route to a sweep-dispatchable
   channel. This sprint authors the pre-check diff; AQ2.5 authors the
   classifier — exactly one owner per file. This covers restarts (in-memory `RuntimeHealth`
   resets), missed transitions, and failed graft handoffs by the same
   mechanism. One per member per pass. Daemon shutdown cancels and joins
   the task within the daemon deadline.
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
6. Sweep channel pre-check (owned here): the sweep claims only for members
   the AQ2.5/AQ2.6 classifier resolves to `TmuxSteer` or `Graft`; for a
   `HerdrSteer` or bare-CLI member the sweep performs no claim and no attempt
   increment (test double over the classifier seam), and a pending marker for
   either member — should one exist — is never driven to a stuck flag by this
   sweep. AQ2.7 is the sole Herdr claimant.
7. `just test` + daemon integration suite, all three lanes.

## Paths to delete

None.

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows.
- Live evidence: a real tmux-harness member queued 2 messages while busy,
  observed draining one-per-idle-transition; transcript committed.
  **Requires AQ2.5's heartbeat producer** — no in-tree client sends
  `TeamMemberHeartbeatRequest` today, so a live idle transition cannot
  occur without it. The sink/drain tests are exercised by injecting
  heartbeats directly; the sweep pre-check consumes AQ2.5's classifier
  seam (see Dependencies).

## Non-closure / out of scope

- Durable heartbeat history; subscription APIs beyond the internal sink;
  priority ordering; re-nudge/reminder policies.

## Dependencies

- must_follow: AQ1 (kinds, store, suppression) — merge-forward before every
  dev/fix round.
- must_follow: AQ2.5 (the sweep pre-check consumes its `DeliveryChannel`
  classifier seam; the live-evidence validation additionally requires
  its heartbeat producer). Merge-forward trigger: AQ2.5 dev push.
- must_follow: AQ2.6 (the classifier gains the retained-tmux/alternate-Herdr
  distinction; this sprint owns the corresponding "skip Herdr" pre-check
  diff). Merge-forward trigger: AQ2.6 dev push.
- parallel_safe: AQ2 (tmux drain vs graft channel — disjoint emitters).

# Sprint AQ8 — Idle-Drain for Deferred Nudges

Status: draft · Branch: `feature/aq-8-idle-drain` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

When a recipient harness transitions to idle, nudge the NEXT unread queued
message for that member. One nudge per idle transition — a backlog drains at
the harness's own pace.

Verified baseline (integrate/phase-ao2): harness state arrives as
`TeamMemberHeartbeatRequest` (`HeartbeatActivity: ActiveToolUse | Idle |
SessionEnded`, protocol.rs:330-342) recorded by
`RuntimeHealth.record_heartbeat()` (runtime_health.rs:101-158) into an
in-memory `HashMap` (observability, not durable; polling-only — **no
transition event mechanism exists today**). Activity mapping:
ActiveToolUse→Active, Idle→Idle, SessionEnded→Offline.

## Deliverables

1. **Idle-transition hook**: `RuntimeHealth.record_heartbeat()` gains a
   transition notification (callback/channel registered by the runtime) that
   fires when a member's state changes to `Idle`. In-memory, best-effort —
   correctness never depends on it (see deliverable 3).
2. **Drain step**: on an idle transition for member M, query M's oldest
   unread message with `nudge_pending_at NOT NULL` (ULID order — AQ7's
   derived FIFO); if one exists, fire the ordinary received-hook nudge for
   exactly that message-id via the existing `MessageReceivedHookSelector`
   path, and clear its `nudge_pending_at`. **One message per transition.**
   Already-read rows are skipped (their markers were cleared by AQ7).
3. **Recovery sweep**: a low-frequency periodic pass (piggybacking the AQ4
   maintenance cadence pattern) performs the same drain check for members
   currently `Idle` whose pending FIFO is non-empty — covering daemon
   restarts (in-memory `RuntimeHealth` resets) and missed transitions. Same
   one-per-member-per-pass bound.
4. **Observability**: structured event per drained nudge `{member, msg_id}`
   plus mandatory `subsystem`/`action`/`outcome` fields, and a cumulative
   drained-nudge counter on the health report (`queue_full_drops_total`
   precedent). Same recorded `emit_daemon_event` exception as AQ4.

## Normative contract

```rust
/// Fired by RuntimeHealth on a state transition; Idle transitions drive the
/// drain. Best-effort; the recovery sweep is the correctness backstop.
pub trait MemberStateTransitionSink: Send + Sync {
    fn on_transition(&self, member: &MemberKey,
                     from: RuntimeMemberState, to: RuntimeMemberState);
}
```

Drain policy (normative): eligible = unread AND `nudge_pending_at NOT NULL`;
order = message ULID ascending; at most one nudge per member per
idle-transition or recovery pass; firing clears `nudge_pending_at` (a
message is deferred-nudged at most once — if it then sits unread, that is
the same outcome as an ignored immediate nudge today).

## Acceptance criteria

1. Heartbeat Active→Idle with two pending unread messages → exactly the
   oldest is nudged (tmux emitter test double records it) and its marker
   clears; the second drains on the next Idle transition.
2. Message read before its drain → skipped entirely; next pending message
   drains instead.
3. Daemon restart with pending rows → recovery sweep drains for an Idle
   member without any heartbeat transition.
4. No transition, no sweep tick → no nudge (deferral actually defers).
5. Graft recipients: if AQ7 wired the graft queue channel, drain excludes
   them; else they drain identically (matching AQ7's recorded fallback).
6. `just test` all three CI lanes; daemon integration suite green.

## Paths to delete

None.

## Required validation

- `just test` workspace + daemon integration suite, ubuntu + macOS +
  Windows lanes.
- Live evidence: one real tmux-harness member queued 2 messages while busy,
  observed draining one-per-idle-transition; transcript committed.

## Non-closure / out of scope

- Durable heartbeat history; subscription APIs beyond the internal sink;
  priority ordering; re-nudge/reminder policies.

## Dependencies

- must_follow: AQ7 (consumes the pending marker and suppression) —
  merge-forward before every dev/fix round.
- parallel_safe: AQ3, AQ5 (disjoint). AQ4 parallel_safe (both touch daemon
  periodic tasks but own separate tasks; AQ8's sweep reuses the cadence
  pattern, not AQ4's code).

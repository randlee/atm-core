# Sprint AQ2 — Queue: atm-graft Dual-Channel

Status: draft · Branch: `feature/aq-2-queue-graft` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

atm-graft wires BOTH nudge channels independently — steer-shaped
(immediate) and queue-shaped (deferred) — and **where they land is the
harness integration's responsibility**, never atm-core routing. Hermes's
`/steer` + `/queue` integration is complete; no capability detection or
fallback logic in atm-core.

Verified baseline: today's graft path is steer-only —
`PublishedGraftReceivedHook` → `deliver_published_receiver_hook` (bounded
loopback TCP, `GraftPostSendRequest` wire JSON) → receiver-side
`GraftReceiveHook`/`HostNudgeInjector` → `PyNudge` callback. The al3 test
forbids detached receiver-hook work — the queue-channel handoff is a
bounded, synchronous dispatch through the same emit loop (a different wire
message), NOT a background task.

## Deliverables

1. **Queue-shaped channel** per ADR-054 (f): a queue-kind dispatch to a
   graft recipient is handed to the graft receiver over the published
   endpoint as a distinct queue-kind wire message (versioned evolution of
   the `GraftPostSendRequest` contract per ADR-054 (g) — both sides move
   together; the receiver process compat concern is addressed explicitly).
   The steer channel is untouched. Marker cleared on successful handoff
   (`PendingNudgeStore::claim_pending` semantics — handoff is the graft
   recipient's drain).
2. **Handoff failure policy** per ADR-054 (f): on failure,
   `nudge_pending_at` stays set, a structured failure event
   (`subsystem`/`action`/`outcome` + `{member, msg_id}`) is emitted, and a
   cumulative failed-handoff counter appears on the health report
   (`queue_full_drops_total` precedent) — a queued graft message never
   silently loses its nudge. **Ownership**: this sprint owns only the graft
   channel's send-and-report behavior — on failure it reports the failure
   to its caller, which calls AQ1's `PendingNudgeStore::requeue_pending`.
   AQ2 implements no retry scheduling and keeps no attempt state of its
   own; retry eligibility, the attempt count, and the stuck flag all live
   in AQ1's store, and re-dispatch scheduling is AQ3's kind-agnostic sweep.
   That is what keeps AQ2 and AQ3 genuinely parallel-safe.
3. **Python surface**: `PyNudge` (and the hermes-atm runtime callback)
   carries the kind — additive, backward-compatible field per ADR-054 (g);
   `hermes-atm` routes queue-kind to Hermes `/queue` and steer-kind to
   `/steer` (integration already complete on the Hermes side; exact
   contract coordinated with team-lead@atm-dev on M5, framed as an
   atm-graft API addition).
4. **Tests**: emitter test double proves `atm queue` → queue channel and
   `atm send` → steer channel; induced handoff failure leaves the marker,
   emits the event, increments the counter; wire-compat test for the
   evolved graft message (old receiver rejects gracefully or version gate
   per ADR-054 (g)).

## Acceptance criteria

1. Graft recipient: queue-kind handoff on the queue channel, marker cleared
   on success; steer path byte-identical for `atm send`.
2. Failure path: marker retained + structured event + health counter
   (induced-failure test).
3. hermes-atm green with the additive `PyNudge` kind; channel contract
   linked in the PR (M5 coordination recorded).
4. `just test` all three lanes; no detached work introduced (al3 stays
   green).

## Paths to delete

None.

## Required validation

- `just test` + hermes-atm Python tests, all three lanes.
- One live Hermes-fronted demo transcript: `atm queue` lands via `/queue`,
  `atm send` via `/steer`; committed on the branch.

## Non-closure / out of scope

- Tmux drain (AQ3). Send-To/attachments (AQ4).

## Dependencies

- must_follow: AQ1 (taxonomy, kinds, `PendingNudgeStore`) — merge-forward
  before every dev/fix round.
- parallel_safe: AQ3 (graft channel vs tmux drain — disjoint emitters; both
  consume, never redefine, AQ1's store and kinds).

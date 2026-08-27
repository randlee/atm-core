---
status: complete
branch: feature/aq-2-queue-graft
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aq-2-queue-graft
---

# Sprint AQ2 — Queue: atm-graft Dual-Channel

Status: complete · Branch: `feature/aq-2-queue-graft` off `integrate/phase-aq` ·
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
   endpoint (resolved via the AQ1.7 registry lease, not the retired file
   record) as a distinct queue-kind wire message (versioned evolution of
   the `GraftPostSendRequest` contract per ADR-054 (g) — both sides move
   together; the receiver process compat concern is addressed explicitly).
   The steer channel is untouched. Marker cleared on successful handoff
   via `PendingNudgeStore::clear_pending_on_handoff(member, msg)` — the
   specific-message clear AQ1's contract defines for synchronous
   handoffs (never `claim_next_pending`, which selects the OLDEST
   pending message and would clear the wrong marker when a backlog
   exists). Handoff is the graft recipient's drain. **Store handle**:
   `PublishedGraftReceivedHook` already carries the whole
   `LocalServiceRuntime` as a struct field (real code today,
   `received_hook_selector.rs:78`, `graft: PublishedGraftReceivedHook {
   service_runtime }`), so `clear_pending_on_handoff`/failure reporting
   just calls `self.service_runtime.pending_nudge_store()?` — no new
   composition-root plumbing needed.
2. **Handoff failure policy** per ADR-054 (f) — two distinct failure
   paths, because only one of them ever holds a claim:
   - **Write-time handoff** (a `Graft`-classified recipient's dispatch
     reaches this sprint's emitter directly out of `PreparedWrite::finish`,
     with no claim taken): on failure `nudge_pending_at` simply stays set
     — `mark_pending` already ran, and there is no `NudgeClaim` to pass to
     `requeue_pending`, so nothing is requeued and no attempt is
     incremented. AQ3's kind-agnostic sweep (`claim_next_pending`) is what
     retries it later. This sprint calls no `PendingNudgeStore` mutation
     on this path.
   - **Sweep-dispatched handoff** (AQ3 already called `claim_next_pending`
     and routed the resulting claim through the selector to this sprint's
     emitter): on failure this sprint reports the failure to its caller
     (AQ3), which holds the `NudgeClaim` and calls
     `PendingNudgeStore::requeue_pending`. This sprint's own code never
     calls `requeue_pending` — it has no claim to pass.
   Both paths emit the same structured failure event
   (`subsystem`/`action`/`outcome` + `{member, msg_id}`) and increment the
   same cumulative failed-handoff counter on the health report
   (`queue_full_drops_total` precedent) — a queued graft message never
   silently loses its nudge either way. **Ownership**: this sprint owns
   only the graft channel's send-and-report behavior and keeps no attempt
   state of its own; retry eligibility, the attempt count, and the stuck
   flag all live in AQ1's store, and re-dispatch scheduling is AQ3's
   kind-agnostic sweep. That is what keeps AQ2 and AQ3 genuinely
   parallel-safe — AQ2's own code never calls `requeue_pending`.
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
2. Failure path, both flavors: write-time (no claim — marker retained,
   nothing requeued, no attempt increment) and sweep-dispatched (claim
   held by the caller — caller calls `requeue_pending`, AQ2 never does);
   both emit the structured event + health counter (induced-failure test
   for each).
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

- must_follow: AQ1.7 (graft endpoint consumer cutover) — the queue channel
  resolves receiver endpoints via the daemon registry, never the retired
  file record. Merge-forward trigger: AQ1.7 dev push.
- must_follow: AQ1 (taxonomy, kinds, `PendingNudgeStore`) — merge-forward
  before every dev/fix round.
- parallel_safe: none. (The former `parallel_safe: AQ3` was dead text —
  AQ3 must_follow AQ2.5 which must_follow this sprint, so AQ3 transitively
  follows AQ2; critical review I1, removed 2026-08-26. Emitters are still
  disjoint files.)
- Downstream: AQ2.5 must_follow this sprint — it adds
  `PullPendingReceivedHook` and a selector match arm to the same
  `received_hook_selector.rs` this sprint's queue-channel changes touch;
  this sprint's diff lands first (recorded in AQ2.5's Dependencies).

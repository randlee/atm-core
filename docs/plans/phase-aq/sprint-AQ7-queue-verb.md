# Sprint AQ7 — `atm queue`: Deferred-Nudge Send

Status: draft · Branch: `feature/aq-7-queue-verb` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

`atm queue` is `atm send` with exactly one difference: the post-write nudge
is deferred until the recipient harness is ready (AQ8 drains it). The
message itself is written durably and immediately through the unchanged
canonical path — there is no separate queue store to lose.

Verified baseline (integrate/phase-ao2): the steer nudge fires synchronously
immediately post-persistence — `emit_received_hook` call site at
`storage_and_nudge_router.rs:538` (definition at :234), guarded only by
`if committed.newly_persisted`, and that single-call-site property is
mechanically pinned by
`atm-architecture/tests/boundary_enforcement.rs::al3_received_hook_is_single_receiver_side_path_without_detached_work`;
**no deferral surface exists**.
`mail_message_states` has an `ensure_column` migration pattern
(`shared_db.rs:888-935`). The queue channel is defined in **hermes-atm**
(M5 side, wrapping atm-graft): Hermes exposes `/steer` (immediate
injection — what the graft nudge path feeds today) and `/queue` (deferred)
as first-class input channels. It is not present in atm-core's
atm-graft/atm-graft-python crates on the reference tree — the wiring seam
is on the atm-core side of the graft boundary.

## Deliverables

1. **`atm queue` CLI verb**: clap surface mirrors `atm send` exactly
   (positional `to`/`message`, `--attach`, `--from-json`, and the rest of
   the send flag set), sharing the send implementation — one code path with
   a `deferred_nudge` bit, not a fork. Same staging/transfer behavior as
   AQ2; same cancel semantics.
2. **`nudge_pending_at` column** on `mail_message_states` via the existing
   `ensure_column` migration pattern. Set at write time for queued messages;
   cleared when the deferred nudge fires (AQ8) or the message is read first.
   The recipient's pending FIFO is **derived**: unread rows with
   `nudge_pending_at NOT NULL`, ordered by message ULID — restart-safe by
   construction, no in-memory truth.
3. **Steer-nudge suppression — upstream seam, NOT the router call site**:
   the queued-vs-immediate decision is caller-owned state-machine logic per
   ADR-019 and is implemented inside
   `PreparedWrite::build_received_hook_dispatches`
   (`crates/atm-core/src/send/mod.rs:391-417`), which already returns
   `Ok(Vec::new())` for its no-dispatch case — a `NudgeMode::Deferred`
   write simply omits the steer-shaped dispatch (tmux send-keys / graft
   steer-channel) from the returned dispatch set. The
   `storage_and_nudge_router.rs` `emit_received_hook` call site, its
   `if committed.newly_persisted` guard, the `al3_*` architecture test, and
   `boundaries/atm-http-runtime/http-runtime.toml`'s unconditional
   post-write hook invariant are all **untouched** — the emitter still runs
   unconditionally; it just receives no steer dispatch for a deferred
   write. This suppression explicitly does NOT cover deliverable 4's graft
   queue-channel handoff, which is a separate, allowed write-time dispatch
   produced by the same seam.
3a. **Read-path marker clear**: the existing read-state transition (the
   code path that sets `mail_message_states.read = 1` when a message is
   read via `atm read`/the read surface) additionally clears
   `nudge_pending_at` in the same state update — the concrete function is
   identified in the PR the way deliverable 3 names its call site. This is
   the build task behind the "reading clears the marker" invariant and
   AC 3.
4. **Graft dual-channel wiring**: atm-graft wires BOTH channels
   independently — immediate nudges on the existing steer-shaped channel,
   queued nudges on a distinct queue-shaped channel — and **where they land
   is the harness integration's responsibility**, not atm-core's. No
   capability detection, no fallback logic, no per-recipient routing in
   atm-core: a graft recipient's queued nudge always goes out on the queue
   channel (marker cleared on successful handoff), and the harness
   integration (Hermes `/steer` + `/queue`: integration already complete)
   decides delivery. AQ8's idle-drain never touches graft recipients — the
   deferred queue is exclusively a tmux received-hook concern. Coordinate
   the exact channel contract with team-lead@atm-dev on M5 (frame any
   needed surface as an atm-graft API addition per standing practice).

5. **ADR-055 conformance**: every mechanism in this sprint implements
   AQ9's ADR-055 exactly as recorded — taxonomy (a), pending-marker
   semantics (b), suppression seam (c), `PendingNudgeStore` governance (d),
   dual-channel contract and handoff failure policy (f). AQ7 re-opens none
   of them; deviations require an ADR change.
6. **`PendingNudgeStore` storage capability** (authorized by ADR-055 (d)) (owned by `atm-storage`): the
   queue's three storage operations go through one narrow trait — never raw
   SQL above the backend crate (`no_backend_specific_message_contract`
   gate). Ships with the ADR-018 §3 follow-up amendment naming it as the
   newly authorized optional capability trait, a
   `boundaries/atm-storage/pending-nudge-store.toml` record, an
   `atm-architecture` boundary test, and `boundary-guard` review as a merge
   precondition (the `message-store.toml` closed contract list is
   unaffected — this is a separate capability, not a `MessageStore`
   widening).
7. **Observability + handoff failure policy**: the suppression decision and
   the graft queue-channel handoff each emit a structured event with the
   mandatory `subsystem`/`action`/`outcome` fields plus `{member, msg_id}`.
   On queue-channel handoff **failure**, `nudge_pending_at` stays set (so
   recovery/retry can act), a structured failure event is emitted, and a
   cumulative failed-handoff counter appears on the health report
   (`queue_full_drops_total` precedent) — a queued graft message never
   silently loses its nudge.

## Normative contract

```rust
/// Shared by send/queue: the only behavioral fork. Lives in atm-core::send
/// alongside WriteRequest (caller-owned decision, ADR-019).
pub enum NudgeMode { Immediate, Deferred }

/// The ADR-018 §3-authorized storage capability behind the queue (owned by
/// atm-storage; fixed/internal implementation set, ADR-001 sealed-supertrait
/// pattern; sync methods by the same recorded exception as AQ1's traits —
/// async callers use spawn_blocking).
pub trait PendingNudgeStore {
    /// Oldest unread pending message for the member, ULID order.
    fn next_pending(&self, member: &MemberKey)
        -> Result<Option<AtmMessageId>, StorageError>;
    /// Atomic claim: clears nudge_pending_at iff still set and unread;
    /// returns true iff this caller won the claim. THE at-most-once
    /// mechanism (single conditional UPDATE … RETURNING).
    fn claim_pending(&self, member: &MemberKey, msg: &AtmMessageId)
        -> Result<bool, StorageError>;
    /// Read-path clear (same state update that sets read = 1).
    fn clear_pending_on_read(&self, member: &MemberKey, msg: &AtmMessageId)
        -> Result<(), StorageError>;
}
```

Queue rows: `nudge_pending_at` is an ISO timestamp (set = deferred nudge
outstanding). Invariants: a queued message is readable immediately (`atm
read` does not wait for a nudge); reading a message clears its pending
marker; markers survive daemon restart.

**TTL interaction (decided — accepted risk, documented):** staged
attachments for queued messages age under the same unconditional AQ1/AQ4
30-day `$ATM_TEMP` sweep as everything else — deliberately no exemption for
outstanding `nudge_pending_at` rows, because coupling the sweeper to message
state is exactly the machinery this phase removed. A recipient drained
toward a path the sweeper already reclaimed sees an ordinary
missing-file situation, identical to any stale path reference; a message
left unread for 30 days is abandoned by definition. Recorded in AQ6's
open-item register with this rationale.

## Acceptance criteria

1. `atm queue <to> <msg>` delivers a durably readable message immediately
   with **no immediate/steer-shaped nudge** emitted (no tmux send-keys, no
   graft steer-channel emission) — the graft **queue-channel handoff
   (AC 5) is the one allowed write-time channel action** for graft
   recipients; `mail_message_states` row carries `nudge_pending_at` (for
   tmux recipients; cleared on queue-channel handoff for graft per
   deliverable 4).
2. Full-surface parity test: every `atm send` flag combination accepted by
   `atm queue` (shared truth-table with AQ2's tests); `--attach`/`--from-json`
   behave identically apart from nudge deferral.
3. Reading a queued message before any nudge clears its pending marker
   (verified via the state row).
4. Daemon restart with pending rows present → FIFO re-derivable (query test;
   drain behavior itself is AQ8's AC).
5. Graft recipient: `atm queue`'s nudge goes out on the queue channel and
   `atm send`'s on the steer channel (test double records which), marker
   cleared on queue-channel handoff, AQ8 drain never touches graft
   recipients. Channel contract linked in the PR.
6. `just test` all three CI lanes; no clippy warnings in touched crates.

## Paths to delete

None. `atm send` immediate behavior is unchanged.

## Required validation

- `just test` workspace, ubuntu + macOS + Windows CI lanes.
- Focused command + migration tests named in the PR.

## Non-closure / out of scope

- The idle-drain itself (AQ8). Priority ordering beyond ULID FIFO. Nudge
  batching.

## Dependencies

- must_follow: AQ2 (shares the send/staging surface) and AQ9 (taxonomy,
  ADR-055, kind-aware dispatch) — merge-forward before every dev/fix round.
- parallel_safe: AQ3, AQ4, AQ5 (disjoint); AQ8 must_follow AQ7.

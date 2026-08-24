# Sprint AQ7 — `atm queue`: Deferred-Nudge Send

Status: draft · Branch: `feature/aq-7-queue-verb` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

`atm queue` is `atm send` with exactly one difference: the post-write nudge
is deferred until the recipient harness is ready (AQ8 drains it). The
message itself is written durably and immediately through the unchanged
canonical path — there is no separate queue store to lose.

Verified baseline (integrate/phase-ao2): the nudge fires synchronously
immediately post-persistence in `storage_and_nudge_router.rs:556-561`
(`emit_received_hook` after `commit_write`); **no deferral surface exists**.
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
3. **Steer-nudge suppression**: the post-write hook path skips the
   **immediate steer-shaped** notification for queued messages — the tmux
   send-keys nudge and the graft steer-channel emission (one guarded branch
   at the existing `emit_received_hook` call site; delivery/persistence
   unchanged). This suppression explicitly does NOT cover deliverable 4's
   graft queue-channel handoff, which is a separate, allowed write-time
   action.
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

## Normative contract

```rust
/// Shared by send/queue: the only behavioral fork.
pub enum NudgeMode { Immediate, Deferred }
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

- must_follow: AQ2 (shares the send/staging surface) — merge-forward before
  every dev/fix round.
- parallel_safe: AQ3, AQ4, AQ5 (disjoint); AQ8 must_follow AQ7.

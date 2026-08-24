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
3. **Nudge suppression**: the post-write hook path skips the immediate
   `emit_received_hook` for queued messages (one guarded branch at the
   existing call site; delivery/persistence unchanged).
4. **Hermes `/queue` wiring**: for Hermes-fronted graft recipients, a
   queued message's nudge is delivered through hermes-atm's `/queue` input
   channel instead of `/steer` — Hermes owns readiness, so these recipients
   bypass AQ8's idle-drain entirely (no `nudge_pending_at` drain needed;
   the marker is cleared on successful `/queue` handoff). Coordinate the
   exact channel contract with team-lead@atm-dev on M5 (frame any needed
   surface as an atm-graft API addition per standing practice). Graft
   recipients NOT fronted by Hermes `/queue` (older harnesses) fall back to
   the AQ8 idle-drain like tmux — the routing decision is per-recipient
   capability, recorded in the PR.

## Normative contract

```rust
/// Shared by send/queue: the only behavioral fork.
pub enum NudgeMode { Immediate, Deferred }
```

Queue rows: `nudge_pending_at` is an ISO timestamp (set = deferred nudge
outstanding). Invariants: a queued message is readable immediately (`atm
read` does not wait for a nudge); reading a message clears its pending
marker; markers survive daemon restart.

## Acceptance criteria

1. `atm queue <to> <msg>` delivers a durably readable message immediately
   with **no** tmux/graft nudge emitted; `mail_message_states` row carries
   `nudge_pending_at`.
2. Full-surface parity test: every `atm send` flag combination accepted by
   `atm queue` (shared truth-table with AQ2's tests); `--attach`/`--from-json`
   behave identically apart from nudge deferral.
3. Reading a queued message before any nudge clears its pending marker
   (verified via the state row).
4. Daemon restart with pending rows present → FIFO re-derivable (query test;
   drain behavior itself is AQ8's AC).
5. Hermes-fronted recipient: `atm queue` hands the nudge to `/queue` (test
   double records the channel), marker cleared on handoff, AQ8 drain never
   touches it; non-Hermes graft recipient falls back to the drain. Channel
   contract linked in the PR.
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

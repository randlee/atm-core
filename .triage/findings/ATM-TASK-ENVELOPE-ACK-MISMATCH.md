# ATM-TASK-ENVELOPE-ACK-MISMATCH: task-envelope messages silently get requires_ack=false

## Pattern
```
requires_ack
task_id.is_some()
resolve_acknowledgement_write
resolve_received_acknowledgement_write
ensure_ack_is_pending
```

## Crates Affected
`atm-core` (`crates/atm-core/src/send/mod.rs`, `crates/atm-core/src/ack/mod.rs`) — core ACK protocol, not a crate/Rust defect confined to any phase deliverable. Cross-cutting: affects same-team and cross-team messaging equally. Filed here (legacy cross-cutting tier) rather than under `.triage/phase-*/findings/` because this is a defect in the ATM CLI/protocol itself, not a phase-AI sprint deliverable.

## Sprint Origin
Discovered 2026-07-28 during AI.31/AI.32 integration testing: Cipher-311d attempted to `atm ack` two task-dispatch messages (one of which, `01KYKTEPHZ98QRH5YQ8G858X69`, was a same-team team-lead→Cipher-311d dispatch, not cross-team) and both failed with "message is not pending acknowledgement." Initially misdiagnosed by arch-ctm as a cross-team-routing/test-context-only issue; independently re-investigated and the routing-works claim confirmed correct, but the underlying sender-side gap is real and unaddressed by that framing.

## Status
open — fix dispatched to arch-ctm (2026-07-28), pending correct fix + regression test. arch-ctm's first attempt (commit `4911bb46` on `fix/cross-team-ack-pending-queue`) only re-proved that a genuine `requires_ack=true` cross-team message already acks correctly — it does not reproduce or fix the actual failure mode and was rejected by team-lead.

## Description
Confirmed root cause (traced by Cipher-311d, independently re-verified):

`crates/atm-core/src/send/mod.rs:548` — `let requires_ack = request.requires_ack || task_id.is_some();`

`requires_ack` is only ever set true when the caller explicitly passes `--requires-ack` or populates the CLI's structured `task_id` field. A hand-composed `<atm-task>`/task-envelope message body sent via the plain `atm send --file <path> --team <team>` path — with neither flag set — silently gets `requires_ack=false`. The recipient never gets a `pending_ack` row (`ensure_ack_is_pending` in `crates/atm-core/src/ack/mod.rs` requires `source.pending_ack_at` to be set), so any later `atm ack <id>` on that message legitimately, deterministically fails with "message is not pending acknowledgement."

This reproduces identically for same-team and cross-team sends (`send/mod.rs:548` has no team-scoping) — the cross-team case only surfaced first because that's where it was tested. `resolve_acknowledgement_write` (same-team) and `resolve_received_acknowledgement_write` (peer/cross-team ingress) both hit the same `ensure_ack_is_pending` check.

Team convention (`docs/team-protocol.md`, ack→work→completion) and every hand-authored task dispatch in this project's workflow expects the recipient to ack task envelopes — but nothing in `atm send` enforces or infers that expectation from message content. The result is a class of messages that look like they require ack (by team convention / by containing a task body) but silently don't, and predictably break `atm ack` for any agent following the stated protocol.

One unconfirmed sub-claim from the original report — that ATM's own console/message-rendering layer unconditionally displays an `<action>ack the message</action>` instruction regardless of `requires_ack` — was investigated and NOT confirmed in `crates/`: no literal match found for that instruction text anywhere in the crate source or `docs/team-protocol.md`. If such rendering exists, it lives outside `crates/` (e.g. a skill/hook template) and needs separate identification; it should not be treated as confirmed fact in the fix.

**Recommended fix** (per independent investigation): `atm send` should infer `requires_ack=true` whenever the message body contains a recognized task/nudge envelope (e.g. `<atm-task>`, `<atm from=... message-id=...>`) even without the explicit `--requires-ack`/`task_id` flag — this closes the actual gap without depending on an unconfirmed rendering-layer fix. **Mandatory regression test**: send a task-envelope message via the plain `--file` path without any explicit ack flag, confirm the resulting message has `requires_ack=true` and the recipient can ack it successfully — this reproduces the original failure exactly, unlike arch-ctm's first (rejected) commit which only tested the already-working explicit-`requires_ack=true` path.

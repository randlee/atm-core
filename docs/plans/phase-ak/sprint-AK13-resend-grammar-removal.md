---
title: AK.13 Resend grammar removal + mechanical gate
status: proposed
branch: feature/pak-s13-resend-grammar-removal
worktree: ../atm-core-worktrees/feature/pak-s13-resend-grammar-removal
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.12 merged to integrate/phase-ak
merge_gate: AK.12 merge commit
parallel_safe: false
quality_findings: [AK-MANDATE-EXT-DRIFTED, AK-MANDATE-BOUNDARY-STALE]
---

# AK.13 — Resend grammar removal + mechanical gate

## Closure

Closes F4 and F6 from
`docs/plans/phase-ak/phase-ak-mandate-compliance-fix-scope.md`. AK.11
retired `PeerResendScheduler` to a compile-failing tombstone; with resend
retired there is no production sender of `messages[]`. This sprint is
deletion-dominant: remove the dead grammar for real, and convert the
enforcement artifacts that still describe the retired coordinator as active
into mechanical gates that would catch its reintroduction.

Frame this as **forgot-to-delete**, not a design change — resend was already
retired in AK.11; this sprint finishes removing what it left behind.

## Deliverables

1. Delete `crates/atm-core/src/send/peer_array.rs`.
2. Delete the `messages[]` branch of the peer decoder (should already be
   thin after AK.12's ingress unification) and `ApiRequest::PeerMessages`
   routing in `crates/atm-daemon/src/runtime_health/request_router.rs`.
3. Delete `crates/atm-daemon/src/tests/peer_message_array.rs`.
4. Delete the resend accessors left in `peer_config_store` beyond the
   compatibility `set false` surface AK.11 keeps (that surface stays —
   `set true` must keep being rejected).
5. Delete the `PeerResendScheduler` tombstone module itself
   (`crates/atm-daemon/src/runtime_health/peer_resend_scheduler.rs`) —
   AK.11 only retired it to a `#[deprecated]` stub; this sprint removes it.
6. Delete `boundaries/atm-daemon/peer-resend-scheduler.toml` in the **same
   commit** as step 5, not after.
7. Convert `crates/atm-architecture/tests/boundary_enforcement.rs`'s
   `peer_resend_scheduler_direct_calls` (currently a bare counter) into a
   forbidding assertion: fail the build if any type/module under
   `atm-daemon/src/runtime_health/**` or `peer_http_listener.rs` matches
   `*Scheduler|*Coordinator|*Worker(?!_pool)|*Manager`, or if
   `peer_delivery_router.rs` dispatch branches on anything besides the
   single shared send function.
8. Prove the new gate actually fires: add a temporary/throwaway type
   matching the forbidden pattern in a test fixture, confirm the lint fails,
   then remove the fixture before merge (document this in the PR — it's the
   proof the gate isn't just decorative).

## Explicit prohibitions

- If recovery/resend is ever revived, it re-enters under the original
  mandate allowance (default-OFF, one timer-driven state machine,
  `messages[]` to the same endpoint) as a **new approved sprint** — not by
  reverting this one or leaving scaffolding "just in case."
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs` (F0
  guardrail — operator-directed quarantine, never in scope for any branch
  in this fix scope).
- No new coordinator/scheduler/worker/manager type anywhere on the peer
  send or receive path — this is the exact pattern this sprint's own gate
  now forbids.

## Required validation

- `just lint` (the new forbidding gate must run and pass on the final tree).
- `just test`, `just smoke localhost`, `just smoke local-ip`.
- Line-count delta should be strongly negative; report it in the PR.
- PR evidence includes the gate-fires-then-passes proof from deliverable 8.

## Dependencies

Starts after AK.12 merges (ingress unification must land first since the
peer decoder this sprint prunes is what AK.12 touches). AK.14 (ADR/
requirements reconciliation) should follow this sprint so its text
describes the actually-final state rather than an intermediate one.

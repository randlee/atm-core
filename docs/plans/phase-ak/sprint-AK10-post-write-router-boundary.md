---
title: AK.10 Post-write router boundary closure
status: in_progress
branch: feature/pak-s10-post-write-router-boundary
worktree: ../atm-core-worktrees/feature/pak-s10-post-write-router-boundary
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.6 code merged to integrate/phase-ak and AK.9 code merged to integrate/phase-ak
merge_gate: accepted AK.6 merge commit and accepted AK.9 merge commit
parallel_safe: false
quality_findings: [AK5-BOUNDARY-DRIFT-001]
---

# AK.10 — post-write router boundary closure

## Closure

Close `AK5-BOUNDARY-DRIFT-001` against the final AK.9 line by comparing
`boundaries/atm-daemon/post-write-router.toml` directly with
`crates/atm-daemon/src/runtime_health/peer_delivery_router.rs`. The obsolete
`socket_io` claim has already been removed from the integration baseline; this
sprint must resolve the remaining contract drift rather than re-litigating the
old finding text.

The final boundary record must accurately state the three router outcomes:

1. an inbound peer receipt signals ordinary local post-write/nudge work and
   returns success;
2. a host-qualified origin invokes the one AK.9 direct batch sender and may
   return a typed unconfirmed-delivery error, without scheduling a local nudge;
3. a hostless origin signals ordinary local post-write/nudge work and returns
   success.

This is a boundary-and-proof sprint, not a new routing design. It must not
introduce a coordinator, queue, sender abstraction, or second receive path.

## Type and boundary inventory

| Item | AK.10 role |
| --- | --- |
| `PostWriteRouter::dispatch` | Existing sole post-persistence route selector. Its three existing branch outcomes are asserted and documented; it is not split or replaced. |
| `send_peer_http_batch` | AK.9's free-function sole host-qualified outbound operation. AK.10 names it in the boundary record and proves no second send path is selected. |
| `PostCommitWorkKey` | Existing local receiver/hostless signal. It remains absent from a successful or failed host-qualified-send branch. |
| `AtmErrorCode::RemoteDeliveryUnconfirmed` | Permitted host-qualified stable error code. The boundary record must not claim `error_types = ["none"]` while this path propagates that error. |

No new public type, trait, thread, task, channel, queue, listener, persistence
table, or route is authorized.

## Deliverables

1. Produce a source-to-boundary comparison in the PR that identifies every
   `io_owns`, `io_forbidden`, request/response/error contract, and route note
   in `post-write-router.toml` against the final AK.9 router. Amend the TOML
   only where the source comparison requires it. In particular, distinguish
   the host-qualified direct batch request from local post-write signals and
   declare the actual typed error behavior.
2. Add an executable source-level or focused integration guard covering all
   three outcomes above. It must prove host-qualified sends use the shared
   AK.9 batch sender exactly once and do not enqueue/signal a local nudge;
   peer-receipt and hostless writes must prove the ordinary local signal.
3. Confirm the boundary lint has no stale ownership or forbidden-I/O claim.
   The permitted direct HTTP ownership must be no broader than the configured
   batch send; SQLite remains outside the router, and TLS, DNS worker,
   fallback, graft delivery, and hook execution remain forbidden.
4. In the PR, close `AK5-BOUNDARY-DRIFT-001` only with links to the direct
   comparison and executable proof. If the actual AK.9 implementation differs,
   update this sprint plan before changing behavior.

## Explicit prohibitions

- No `PeerDrainCoordinator`, per-message delivery loop, extra retry policy,
  broad peer scan, or local nudge fallback for a host-qualified send.
- No boundary-file-only closure: the record and a direct executable proof must
  agree with the implementation.
- No claim that a receiver nudge can alter host-qualified sender confirmation
  or error handling.

## Required validation

- `just lint` validates the amended boundary record and dependency edges.
- Focused tests prove the three branch outcomes, one batch request for the
  host-qualified path, no host-qualified local post-write signal, and typed
  propagation of a direct-delivery failure.
- `just test`, `just smoke localhost`, and `just smoke local-ip` pass.
- PR evidence links the final AK.9 commit, the source-to-boundary comparison,
  and the focused proof before the triage finding changes state.

## Dependencies

AK.6 must merge before AK.10 begins. Start AK.10 after AK.9 is pushed, but
merge both AK.6 and AK.9 before every AK.10 development/fix round and before
PR completion. AK.11 does not start until AK.10 itself merges.

---
title: AL.8 Daemon Composition and Static Boundary Proof
status: complete
branch: feature/pal-s8-daemon-composition-proof
worktree: ../atm-core-worktrees/feature/pal-s8-daemon-composition-proof
target: integrate/phase-al
---

# AL.8 — Daemon Composition and Static Boundary Proof

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.3, AL.5, and AL.6. Merge each parent's pushed integration
commit before each development/fix round; parent PR merges are not required.
AL.7's peer TLS work is deferred: TLS is not MVP scope and the repository's
isolated TLS crate remains the only future TLS implementation source. AL.8
must not add, activate, or prove a TLS adapter.
**unblocks:** AL.9 only. AM deletion remains blocked on AL.9 acceptance.
**parallel_safe:** AM.1 inventory only.

**traceability:** `REQ-P-RUNTIME-001`–`006`,
`REQ-P-DAEMON-DISPATCHER-001`, `REQ-DAEMON-RUNTIME-001/002/003`,
`REQ-CORE-TRANSPORT-005`, ADR-026, ADR-036; every runtime proof row in the
shared traceability record.

## Deliverables

1. Activate `atm-http-runtime` as the sole `atm-daemon` process: retain or
   transplant the existing owner gate, create backend-neutral trait
   implementations, inject the accepted `MessageReceivedHookEmitter`, select
   enabled adapters, start the runtime, and perform bounded shutdown. Do not
   start, wrap, test through, or retain the reference-only legacy
   `crates/atm-daemon` server as fallback.
2. Prove no `atm-daemon` or `atm-http-runtime` source/dependency references
   concrete SQLite/Rusqlite, tmux, `atm-graft`, raw HTTP framing, peer-only
   application code, or resend/replay.
3. Capture the actual source-level live-reference graph as input to AM.1's
   ledger. Do not infer deletions from stale documentation and do not freeze
   the ledger here.
4. Publish runtime health through the existing daemon status/readiness surface,
   not a second server: `Ready` only after owner gate, AL.1 validation, router
  construction, and every enabled MVP local listener bind succeeds; `NotReady` before
   start and throughout drain; `Live` reflects process/runtime supervision.
   A failed bind/TLS/configuration start remains `NotReady` with a typed cause.
5. Adopt the architecture contract's **5s** daemon graceful-drain deadline
   (`docs/architecture.md` §21.6.4). AL.8 reconciles the legacy transport's
   differing local constant as part of the one runtime cutover rather than
   leaving two governing deadlines. Shutdown stops accepts first, drains
   tracked requests until that deadline, then
   cancels remaining work and transitions readiness to `NotReady`; it never
   extends the deadline with detached helpers or background work.

## Acceptance criteria

- The active daemon starts only after its existing singleton gate and publishes
  no listener earlier; shutdown stops accepts and drains/cancels tracked work
  within the documented bound.
- AL.8 activates only the framework-managed Unix UDS (where supported) and
  loopback TCP adapters. Peer TLS remains deferred and cannot become an
  implicit startup dependency.
- The static route/composition trace identifies the common handler, storage
  trait, and received-hook call site without a second listener/client root.
- Boundary searches prove no raw framing, peer-only ingress, replay, concrete
  SQLite, tmux, or graft implementation dependency in daemon/runtime code.
- New/duplicate/hook-failure behavior reaches the common handler in
  deterministic in-process tests.
- Health/readiness transition tests prove no `Ready` state before all enabled
  listeners bind and `NotReady` on failed start/drain; shutdown completes or
  cancels within the retained 5s deadline.

## Required validation

- `just test`, formatter, lint, dependency/boundary checks
- in-process composition, lifecycle drain/cancel, no-direct-SQL, and
  public-schema snapshot tests
- failed-start/readiness and bounded 5s shutdown-drain transition tests
- independent checklist review and live-reference graph review

## Non-closure

AL.8 authorizes AL.9 physical proof only. It does not authorize AM deletion,
delete legacy source, or add future recovery/replay.

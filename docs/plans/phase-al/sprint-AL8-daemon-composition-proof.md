# AL.8 — Daemon Composition and Static Boundary Proof

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.3, AL.5, AL.6, and AL.7. Merge each parent's pushed
integration commit before each development/fix round; parent PR merges are not
required.
**unblocks:** AL.9 only. AM deletion remains blocked on AL.9 acceptance.
**parallel_safe:** AM.1 inventory only.

**traceability:** `REQ-P-RUNTIME-001`–`006`,
`REQ-P-DAEMON-DISPATCHER-001`, `REQ-DAEMON-RUNTIME-001/002/003`,
`REQ-CORE-TRANSPORT-005`, ADR-026, ADR-036; every runtime proof row in the
shared traceability record.

## Deliverables

1. Make `atm-daemon` a composition/lifecycle root only: retain existing owner
   gate, create backend-neutral trait implementations, inject the accepted
   `MessageReceivedHookEmitter`, select enabled adapters, start the runtime,
   and perform bounded shutdown.
2. Prove no `atm-daemon` or `atm-http-runtime` source/dependency references
   concrete SQLite/Rusqlite, tmux, `atm-graft`, raw HTTP framing, peer-only
   application code, or resend/replay.
3. Capture the actual source-level live-reference graph as input to AM.1's
   ledger. Do not infer deletions from stale documentation and do not freeze
   the ledger here.

## Acceptance criteria

- The active daemon starts only after its existing singleton gate and publishes
  no listener earlier; shutdown stops accepts and drains/cancels tracked work
  within the documented bound.
- The static route/composition trace identifies the common handler, storage
  trait, and received-hook call site without a second listener/client root.
- Boundary searches prove no raw framing, peer-only ingress, replay, concrete
  SQLite, tmux, or graft implementation dependency in daemon/runtime code.
- New/duplicate/hook-failure behavior reaches the common handler in
  deterministic in-process tests.

## Required validation

- `just test`, formatter, lint, dependency/boundary checks
- in-process composition, lifecycle drain/cancel, no-direct-SQL, and
  public-schema snapshot tests
- independent checklist review and live-reference graph review

## Non-closure

AL.8 authorizes AL.9 physical proof only. It does not authorize AM deletion,
delete legacy source, or add future recovery/replay.

# AL.8 — Daemon Composition, Combined Proof, and Performance Gate

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.3, AL.5, AL.6, and AL.7.
**unblocks:** AM.2–AM.6.
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
3. Capture the combined active-path evidence: in-process, Unix UDS, loopback,
   localhost/advertised same-host TLS, and M5 cross-host all use the AL router,
   handler, storage trait, and received-hook path.
4. Record a reproducible pre-AL versus AL benchmark environment, command,
   raw result, and comparison. Investigate any material regression before AM.
5. Freeze the actual remaining legacy references as AM.1's removal-ledger
   input; do not infer deletion candidates from stale documentation.

## Acceptance criteria

- The active daemon starts only after its existing singleton gate and publishes
  no listener earlier; shutdown stops accepts and drains/cancels tracked work
  within the documented bound.
- The unchanged public request/result/error JSON snapshots pass for every
  adapter.
- New/duplicate/hook-failure semantics pass once through the common handler.
- Direct remote failure creates no retry/replay task and no old compatibility
  listener/client remains in the active path.

## Required validation

- `just test`, formatter, lint, dependency/boundary checks
- active-daemon local UDS/loopback/same-host smoke and M5 clean-checkout smoke
- lifecycle drain/cancel, no-direct-SQL, and public-schema snapshot tests
- benchmark artifact and independent checklist review

## Non-closure

AL.8 authorizes AM deletion only. It does not delete legacy source or add
future recovery/replay.

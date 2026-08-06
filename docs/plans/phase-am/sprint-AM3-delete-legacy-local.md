# AM.3 — Delete Legacy Local Ingress and Egress

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.9, AM.1, and the frozen ledger's designated predecessor
(normally AM.2). All named predecessor deletion PRs must be merged before this
PR begins; AM.3/AM.4 order is the ledger topology, not their number.
**unblocks:** AM.5 and AM.6.
**parallel_safe:** none; deletion changes active local composition.

**traceability:** `REQ-CORE-TRANSPORT-001/001B`,
`REQ-DAEMON-TRANSPORT-001/005/006/008`, ADR-033.

## Deliverables

1. Delete superseded local UDS and loopback client/listener workers, manual
   accept/read/write loops, module exports, fixtures, docs, and dependencies
   named by the AM.1 ledger.
2. Leave the AL.5/AL.6 framework adapters as the only local physical setup;
   do not retain a fallback client/server for safety.
3. Enable the relevant AM.1 negative guards in the same deletion PR once the
   listed symbols are absent.

## Acceptance criteria

- A repository call graph finds one active local client and listener path,
  both inside `atm-http-runtime`.
- UDS and loopback preserve the AL.1 public JSON snapshot and common handler
  trace; local smoke passes on supported targets.
- No raw local framing, OS-specific application route, or direct storage call
  survives.

## Required validation

- full test/format/lint suite
- UDS and loopback smoke, parity tests, and static negative guards
- targeted mutation proving reintroduced legacy local symbol fails a guard

## Non-closure

Peer ingress/egress and replay deletion remain AM.4 and AM.5.

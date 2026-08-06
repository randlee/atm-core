# AM.6 — Minimality Audit and Completion Proof

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** every frozen-ledger deletion owner (including AM.3, AM.4, and
AM.5) has merged. This is a PR-completion gate, not a merge-forward gate.
**unblocks:** no implementation sprint; this is the Phase AM exit gate.
**parallel_safe:** none.

**traceability:** every shared checklist and traceability row, especially
`REQ-CORE-BOUNDARY-001/002`, `REQ-DAEMON-RUNTIME-002`, ADR-001, ADR-032,
ADR-033, and ADR-036.

## Deliverables

1. Close every AM.1 ledger row with source-level evidence and verify all
   enabled negative guards against an actual mutation.
2. Audit each remaining `atm-daemon`/`atm-http-runtime` module: daemon is
   composition/lifecycle only; runtime is the sole maintained HTTP client and
   server implementation; `atm-core` owns application contracts and storage
   traits.
3. Re-run the full AL.9 proof suite and compare final benchmark with its raw
   baseline/result artifact.
4. Produce QA handoff naming the one public type/schema oracle, client
   implementation, router, `ApiRouter` dispatch, `MessageWriter` boundary, and
   received-hook call site.

## Acceptance criteria

- Legacy modules, dependencies, symbols, tests, docs, and compatibility paths
  are absent, not merely unused.
- Existing public transport structs and JSON serialization remain unchanged.
- All physical adapters prove the one direct path; no automatic replay or peer
  divergence survives.

## Required validation

- `just test`, formatter, lint, dependency graph, and source/guard mutation
  suite
- local UDS/loopback/same-host smoke, M5 cross-host smoke, and benchmark
- independent QA review against the checklist, runtime design, and
  traceability record

## Non-closure

AM does not add retransmission, storage behavior, notification UX, or an API
schema change. Its only completed outcome is a smaller runtime with the same
contract.

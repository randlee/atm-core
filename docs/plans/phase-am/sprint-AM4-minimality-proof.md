# AM.4 — Minimality Audit and Completion Proof

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AM.2 and AM.3.
**unblocks:** no implementation sprint; this is the phase exit gate.
**parallel_safe:** none.

## Deliverables

1. Audit remaining `atm-daemon` and `atm-http-runtime` modules against the
   shared boundary checklist.
2. Close the deletion ledger with source-level evidence for every row.
3. Run full validation and compare the final benchmark with AL.5's recorded
   baseline.
4. Produce QA handoff that names the sole client, listener, handler, and
   received-hook call site.

## Acceptance criteria

- The daemon only composes/injects/starts/stops the runtime.
- `atm-http-runtime` is the only HTTP server/client implementation.
- There is one typed ingress and one received-hook call path after a new write.
- All shared checklist proof items pass, and no compatibility path survives.

## Required validation

- `just test`, formatter, lint, local smoke, M5 smoke, and benchmark evidence
- source search and mutation proof for every prohibited category
- independent QA review directly against the AL/AM sprint docs and checklist

## Non-closure

AM does not add a retransmission feature, new storage behavior, or new
notification UX. Its only successful outcome is a smaller runtime with the
same shared contract.

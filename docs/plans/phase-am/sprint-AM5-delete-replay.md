---
status: complete
branch: feature/pam-s5-delete-replay
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pam-s5-delete-replay
---

# AM.5 — Delete Recovery and Replay Complexity

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** the frozen ledger's designated completed predecessors (at
least AM.3 and AM.4 when they own callers). Those deletion PRs must be merged
before this PR begins; no numeric ordering assumption overrides the topology.
ADR-041 hook-latency wording must also be reconciled and accepted before this
deletion begins; an unresolved warning/latency contract is a blocking state.
**unblocks:** AM.6.
**parallel_safe:** none; it owns every residual send-background path.

**traceability:** `REQ-CORE-TRANSPORT-003`,
`REQ-DAEMON-TRANSPORT-002`, and the explicitly deferred `003B` disposition in
the shared traceability record.

## Deliverables

1. Delete resend/replay schedulers, caches, cursors, queues, timers, workers,
   drain/recovery coordinators, configuration/doctor surfaces, and tests that
   exist solely for automatic recovery.
2. Delete compile-failing tombstones once all implementation and callers are
   gone. Finished architecture is absence, not a permanent deprecated API.
3. Retain direct send and canonical idempotent duplicate handling only; neither
   constitutes a retry/replay subsystem.
4. Delete the ledger-confirmed legacy transport observability/capacity/state
   surfaces—at minimum `peer_delivery_observability` if it has no retained
   consumer—and their doctor/config/dashboard entries. Preserve a strict-config
   upgrade disposition for removed replay keys and add a negative guard for
   every removed production symbol. An active request registry is not in scope
   unless the frozen ledger proves it obsolete.

## Acceptance criteria

- A failed direct cross-host send returns its ordinary typed outcome and starts
  no background worker, task, timer, queue, or state machine.
- No send path creates `message[]`, peer-only payload, or alternate endpoint.
- Static guards find no resend/replay/coordinator symbols in production code.

## Required validation

- failure-path integration test with task/worker accounting
- negative symbol/dependency guard and mutation proof
- full test, formatter, and lint suite

## Implementation note

Commit `9eebc607` also rejects a graft received-hook budget that is exhausted
after reserving the result-handoff grace period.  This prevents a zero-budget
hook from beginning socket I/O after its caller can no longer receive a result.

## Non-closure

Future recovery is a separate, explicitly approved phase after minimum
cross-host proof. It must begin from the AL shared endpoint/types.

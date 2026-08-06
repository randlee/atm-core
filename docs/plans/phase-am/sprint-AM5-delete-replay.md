# AM.5 — Delete Recovery and Replay Complexity

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AM.3 and AM.4.
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

## Acceptance criteria

- A failed direct cross-host send returns its ordinary typed outcome and starts
  no background worker, task, timer, queue, or state machine.
- No send path creates `message[]`, peer-only payload, or alternate endpoint.
- Static guards find no resend/replay/coordinator symbols in production code.

## Required validation

- failure-path integration test with task/worker accounting
- negative symbol/dependency guard and mutation proof
- full test, formatter, and lint suite

## Non-closure

Future recovery is a separate, explicitly approved phase after minimum
cross-host proof. It must begin from the AL shared endpoint/types.

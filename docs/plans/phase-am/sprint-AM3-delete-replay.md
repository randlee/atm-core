# AM.3 — Delete Recovery and Replay Complexity

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.5 and AM.1.
**unblocks:** AM.4.
**parallel_safe:** none; it owns every resend/recovery runtime path.

## Paths/categories to delete

- Peer resend/replay schedulers, caches, cursors, queues, timers, workers, and
  drain/recovery coordinators.
- Configuration, doctor/status output, observability, and tests that exist only
  for automatic resend/replay.
- Compile-failing tombstones once the implementation is gone; deletion, not
  permanent deprecation, is the finished state.

## Acceptance criteria

- A failed direct cross-host send returns the normal failure and starts no
  background retry or state machine.
- Static guards find no resend/replay type, timer-driven sender, queue, or
  worker remaining in production code.
- No send path constructs `message[]` or a peer-specific body.

## Required validation

- failure-path integration test with task/worker accounting
- static negative architecture guards
- full test, formatter, and lint suite

## Non-closure

No future replay implementation is designed in this phase. A later authorized
feature must start with a new plan and must use AL's canonical endpoint/types.

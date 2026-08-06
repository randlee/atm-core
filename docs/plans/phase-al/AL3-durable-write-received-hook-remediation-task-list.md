# AL.3 Durable Write and Received-Hook Remediation

Status: proposed follow-up; AL.3 is not closure-ready until every item below is
accepted.

## Contract

One inbound write has one absolute request deadline. The request must not gain
a second, additive hook timeout.

```text
request future
  -> await bounded SQLite write job
  -> committed durable outcome
  -> await receiver-hook operation with the same deadline's remaining budget
  -> successful response, optionally with warnings
```

The SQLite queue contains bounded **write jobs**, not futures. Each job carries
one completion sender; the originating request future awaits that sender. The
received-message hook is likewise an awaited operation of that request, not a
detached task, retry, queue, or background worker.

## Task list

- [x] **AL3-RH-000 — Remove SQLite-failure-as-success fallback.**
  - `SqliteFailedRecovered` currently converts a mailbox write failure into a
    degraded external delivery plus a warning. That conflicts with the durable
    write contract: a response is successful only after SQLite reports a
    committed record (or an idempotent already-recorded row).
  - Propagate database admission/transaction failure as the established
    machine-readable error response. Do not emit the original payload, a
    companion message, or a received hook after that failure.
  - Acceptance: injected `MailboxWriteFailed` produces an error, persists no
    record, emits no hook, and does not send a fallback payload. Retire the
    `SqliteFailedRecovered` execution/disposition path if it has no other
    explicitly approved contract.

- [ ] **AL3-RH-001 — Establish one deadline semantic.**
  - Construct one absolute deadline at ingress and pass it unchanged to queue
    admission, SQLite execution, the received-hook operation, and response
    serialization.
  - Make `RequestDeadline::expired()` and `RequestDeadline::remaining()` use
    one consistent zero-boundary rule: zero remaining time is expired.
  - Acceptance: an exactly-zero deadline cannot start a SQLite job or hook;
    no layer creates a second full-duration deadline.
  - Corner-case tests: exact zero, deadline consumed while waiting for queue
    capacity, deadline consumed after dequeue but before transaction start, and
    a positive-but-small remaining duration.
  - [x] The exact-zero boundary is now consistent: `remaining() == None`
    implies `expired()`, with an explicit core regression test.

- [ ] **AL3-RH-002 — Make SQLite admission an awaited bounded job.**
  - Replace a queue of synchronous dispatch work with a bounded SQLite write
    job queue whose job result is returned through a one-shot completion path
    and awaited by the caller's request future.
  - Preserve SQLite's serialized/synchronous execution inside its owned
    executor; do not run SQLite transactions on Tokio worker threads.
  - Every ingress adapter submits through the same executor. There must be no
    separate UDS, TCP, peer, or per-connection write queue.
  - Define cancellation explicitly: a dropped caller before the job starts
    removes or skips that job without writing; a started transaction completes
    and records its actual result. The system must never tell a still-connected
    caller that a started job failed merely because an outer timer stopped
    awaiting it.
  - Define the commit boundary: a timeout before a job begins is a failure with
    no write. Once a transaction starts, the response must report its actual
    durable outcome rather than report timeout and later commit invisibly.
  - Acceptance: saturation, pre-start timeout, successful commit, database
    failure, and slow in-progress transaction all have deterministic,
    non-contradictory caller outcomes.
  - Corner-case tests: bounded-capacity rejection/backpressure, FIFO or the
    explicitly documented scheduling rule, caller cancellation before start,
    transaction error, duplicate-message idempotency, and transaction start
    immediately before the deadline.
  - **Placement constraint:** do not repurpose or add a daemon-local
    `DispatchWorkerPool`/thread queue for writes. The executor must be owned
    by the one Tokio runtime composition and invoked by every connector before
    the canonical write route; otherwise UDS, loopback, and peer HTTP acquire
    divergent admission behavior.
  - **Executor state machine:** each job is `Queued`, `Started`, or
    `CancelledBeforeStart`. Timeout/cancellation may atomically move only
    `Queued -> CancelledBeforeStart`; the executor alone moves
    `Queued -> Started` immediately before entering SQLite. A caller whose job
    is already `Started` waits for and reports the actual transaction result,
    even when that exceeds the advisory deadline, so no invisible commit can
    be reported as a timeout.

- [ ] **AL3-RH-003 — Make the received hook an awaited, deadline-aware operation.**
  - Invoke it only after a newly committed write; never for an idempotent
    duplicate.
  - Pass the one request deadline (or its remaining duration) into the
    receiver-hook boundary. Its effective limit is
    `min(hook_safety_cap, deadline.remaining())`.
  - Replace blocking sleep/polling with awaitable process/timer operations; do
    not create a detached task or notification queue.
  - The hook boundary remains sealed and object-safe. It must carry a deadline
    or a validated remaining-budget value rather than permit an implementation
    to create a new full request timeout.
  - Acceptance: a deliberately stalled hook stops at the remaining budget and
    cannot occupy an executor after the request completes.
  - Corner-case tests: missing receiver target, emitter construction failure,
    process-start error, non-zero process exit, safety-cap timeout shorter than
    remaining request budget, request deadline shorter than safety cap, and
    idempotent duplicate/no second emission.
  - [x] The sealed `MessageReceivedHookEmitter` boundary now requires the
    inherited `RequestDeadline`; tmux and Graft cap every operation to its
    remaining duration, and a regression test proves a stalled tmux child is
    killed at that budget.
  - [ ] Replace the legacy synchronous process sleep/poll implementation with
    an awaitable runtime operation as part of the bounded-job executor work;
    this remaining item must not be closed by wrapping the synchronous path in
    a detached thread or by creating another full-duration timeout.

- [ ] **AL3-RH-004 — Preserve durable-success response semantics.**
  - A database write failure returns an error response.
  - Once the SQLite transaction reports a committed write, the API result is
    `Sent` or `Acknowledged`; hook start, execution, or timeout failures append
    a caller-visible `WarningEntry` and never turn that durable outcome into an
    error.
  - Remove or narrow any final generic deadline check that can reclassify a
    committed write after the advisory hook has run or been skipped.
  - Acceptance: tests prove (1) database failure is an error, (2) hook error
    is success plus warning, (3) hook timeout is success plus warning, and
    (4) exhausted post-commit hook budget is success plus warning.
  - Verify the warning uses the existing public response schema and a stable
    code/recovery message; do not add a peer-only or hook-specific result
    envelope.
  - [x] Removed the late generic deadline reclassification after the canonical
    write route. A committed write now returns its durable `Sent` or
    `Acknowledged` result; only advisory hook warnings may be appended.
  - [x] Removed the Tokio HTTP adapter's outer timeout around synchronous
    dispatch. A regression test now proves an already-started route returns
    its actual successful response after the advisory deadline instead of a
    synthetic timeout while it continues in the blocking pool.

- [ ] **AL3-RH-005 — Budget and regression proof.**
  - Document the configured end-to-end request deadline per transport and the
    hook safety cap. Do not assume their values are additive.
  - Add tests for queue wait consuming the shared budget, exact-zero boundary,
    a committed write followed by hook timeout, duplicate/no-hook, and no
    detached task remaining after the response.
  - Extend the architecture guard to reject independent full-duration hook
    deadlines and synchronous sleep/poll loops in the production receiver-hook
    path.
  - Acceptance: UDS, loopback/TCP, and peer HTTP use the same outcome matrix;
    the caller receives no false write failure after a reported commit.
  - Add request-cancellation, socket disconnect, and process-cleanup tests so
    the worker/executor has no orphaned task, child process, or durable write
    whose result is unobservable by a still-connected caller.

## Required response matrix

| SQLite outcome | Hook outcome | Caller result |
|---|---|---|
| Fails or is rejected before transaction begins | Not run | Error |
| Commits | Succeeds | Success |
| Commits | Fails to start, fails, or times out | Success + warning |
| Commits idempotent duplicate | Not run | Success; no hook warning |

## Deliberate non-goals

- No sender-side notification, retry, replay, or post-send queue.
- No second listener, peer-only receive route, or schema change solely for hook
  failure.
- No daemon dependency on `atm-graft`; Graft remains an independently started
  receiver implementation injected through the sealed boundary.

# AI.33 Windows admission-backpressure remediation

This is the implementation handoff for cwin on
`feature/pAI-s33-admission-capacity-smoke`. It follows the Windows capacity
sweep through commit `e5955bff`: all tested worker counts still lose response
headers with `WinError 10053`, despite the expanded dynamic port range. The
next step is to repair daemon admission/liveness defects before taking another
performance baseline.

## Scope and non-goals

- Work in the fastpc4 worktree and commit each independently testable repair
  to this branch.
- cwin is authorized to stop and replace the daemon and disposable test
  database on fastpc4.
- Do not change the capacity target, `MAX_CONCURRENT_CONNECTIONS`, worker
  count, queue sizes, deadlines, or add retries to make a result appear to
  pass. A retry of a side-effecting send can duplicate an immutable message.
- Do not classify the remaining `10053` result as an environment limitation
  until the two liveness defects below are fixed and the prescribed baseline is
  rerun.

## Fix 1 — capacity backpressure must yield to shutdown

### Defect

`wait_for_connection_capacity` in
`crates/atm-daemon/src/local_tcp_transport.rs` waits while the active count is
at `MAX_CONCURRENT_CONNECTIONS`. It does not observe
`lifecycle.terminate_requested()`. A wedged handler can therefore hold the
accept loop inside its wait indefinitely, preventing the normal shutdown path
from starting its graceful drain and eventual forced cancellation.

### Required behavior

1. Keep bounded, non-busy waiting for an active-connection change.
2. Check termination before entering the capacity wait and after every timed or
   signalled wait.
3. On termination, return control to the existing top-level shutdown path;
   that path remains the sole owner of `begin_shutdown`, graceful drain, force
   cancellation, endpoint cleanup, and the `Ok(())` return.
4. Do not make `ActiveConnectionRegistry` depend on daemon lifecycle state.
   Pass a cancellation predicate/status into a small transport-local helper or
   recheck lifecycle at the caller boundary instead.

### Deterministic regression test

Add a Windows-gated test using an injected or test-visible capacity-wait
helper. Fill the registry to the cap with a deliberately held connection,
request termination, and assert the wait returns promptly **without releasing
the held worker**. Then assert the normal serve shutdown hook/drain path is
entered. The test must not use arbitrary sleeps as its oracle; use a barrier,
channel, or an explicit predicate transition.

Run this test repeatedly with the normal local test suite. Its assertion is
liveness during overload, not a timing benchmark.

## Fix 2 — dispatch-handle bookkeeping saturation must not kill the daemon

### Defect

`ActiveConnectionRegistry::push_dispatch_handle` synchronously joins a new
handler and returns `Err` when its bounded handle table is full. The Windows
accept loop propagates that `Err` from `spawn_connection` with `?`, terminating
the entire local HTTP serving loop. This is unacceptable: a bookkeeping bound
must affect only the current connection, never make a healthy daemon stop
accepting future requests.

### Required behavior

1. Reserve all capacity necessary to track a worker **before** creating the
   worker, or use another design that retains a shutdown/reap owner for every
   spawned worker.
2. Never block the accept loop in `JoinHandle::join()` because the bookkeeping
   table is full.
3. On saturated admission/tracking, apply a per-connection outcome only:
   return the existing typed saturation response when a request can be
   responded to safely, or close that one connection safely. The outer serving
   loop must continue.
4. Preserve exactly-once active-connection release and bounded shutdown
   tracking. Do not detach an untracked handler merely to avoid the error.
5. Keep handler failures observable in logs and preserve the existing
   opportunistic reap behavior for completed workers.

### Deterministic regression test

Make the handle-table limit injectable in a Windows-gated test. Hold the first
handler with a barrier so the table is full, submit a second connection, and
prove all of the following:

1. the second connection receives the documented saturated result or safe
   connection-close outcome;
2. the daemon serving loop remains alive while the first handler is held;
3. after releasing/reaping the first handler, a subsequent request succeeds;
4. the active-count and tracked-handle counts return to zero after cleanup.

Use barriers/channels, not `sleep`, to establish the full-table state. A unit
test that only checks `write_saturated_response` is insufficient because it
does not prove the accept loop survives the bookkeeping path.

## Integration and capacity revalidation

After each fix:

1. `cargo fmt --all --check`
2. `just lint`
3. `just test`
4. `just smoke localhost`

Then rebuild the exact branch CLI and daemon and run the official Windows
capacity runner from a disposable runner-owned database:

```powershell
$env:ATM_CAPACITY_ISOLATED_OS_USER = '1'
python scripts/smoke/run_admission_capacity.py
```

Record the commit, process/listener identity, doctor output, daemon stdout and
stderr, JSON result, accepted writes, HTTP responses, failures, elapsed time,
and slowest interval. Preserve the twenty-interval accepting/unavailable-peer
workload. If `10053` remains after both repairs, collect that evidence before
investigating listener/backlog or client socket behavior; do not mask it with a
retry or looser gate.

## Commit and handoff discipline

- Commit Fix 1 and Fix 2 separately with their deterministic tests.
- Append a concise dated outcome to
  `ai33-windows-validation-handoff.md` after the integrated capacity rerun.
- Push every commit. Mac-side review will pull the exact range, run `just
  test`, and request independent quality review before accepting it.

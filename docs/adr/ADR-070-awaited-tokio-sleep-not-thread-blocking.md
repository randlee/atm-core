# ADR-070 — An Awaited `tokio::time::sleep` Is Not A Thread-Blocking Defect

| Field | Value |
| --- | --- |
| ID | ADR-070 |
| Status | Accepted |
| Date | 2026-11-18 |
| Scope | Production use of `tokio::time::sleep` and `sleep_until`; the sc-lint `SCB-RUNTIME-005` allowances that carry it |
| Relates to | sc-lint 0.8.9 runtime rules (`SCB-RUNTIME-003` to `-006`); ADR-008 and REQ-P-TEST-001 (no sleeps in tests, unchanged); phase-bg remediation plan (rule R5) |

## Context

sc-lint 0.8.9 `SCB-RUNTIME-005` flags `tokio::time::sleep` and `sleep_until` in all non-test source. It treats them the same way `SCB-RUNTIME-003` treats `std::thread::sleep`. They are not the same:
- `std::thread::sleep` parks the OS thread. On an async worker thread, it stalls every task scheduled there.
- An awaited `tokio::time::sleep` returns a future. The task yields, the thread keeps running other tasks, and tokio's timer wakes the task when the time is up. No thread is blocked.

Rand's rulings (2026-11-18, verbatim):
- "blocking the thread is what must be avoided."
- "add an exception and flag w/ an ADR : awaited tokio::time::sleep should not be flagged by sc-lint."

## Decision

### D1. Thread blocking stays banned
These remain defects, with no exception:
- `std::thread::sleep` and `sleep_until` in async code (`SCB-RUNTIME-003`, `-004`);
- `block_on` on a runtime-owned thread, or nested inside async code (`SCB-RUNTIME-006`);
- any blocking wait on an executor thread, or a sleep or poll loop on a `spawn_blocking` thread where an async primitive exists.

`block_on` is allowed only once, at the top of a dedicated std thread that owns its runtime.

### D2. An awaited tokio sleep is allowed where the delay is the requirement
An awaited `tokio::time::sleep` or `sleep_until` is allowed in production code where the delay is the behaviour itself, such as protocol spacing or retry backoff. It is subject to three conditions:
- **Cancellable.** It runs in `tokio::select!` against shutdown or cancellation, or inside a future its owner cancels.
- **Not a missing event.** Where a real event exists (`Notify`, `watch`, `mpsc`, `oneshot`, `JoinSet`, `child.wait()`), the code waits on the event. Review enforces this, not the lint.
- **The right primitive.** Periodic work uses `interval`/`interval_at`. A deadline on an awaited operation uses `timeout_at`. A sleep is not renamed or wrapped just to silence the lint, and `timeout(d, pending())` is banned.

### D3. The exception, until sc-lint stops flagging it
Each such site carries the allowance below. The reason must name ADR-070, the site's requirement and its cancellation path:

```rust
#[sc_lint_attributes::sc_lint(
    allow(SCB-RUNTIME-005),
    reason = "ADR-070: <requirement, e.g. EMFILE accept backoff>; cancelled by <path>"
)]
```

An allowance that does not cite ADR-070, or a site that fails D2, is a review finding.

### D4. Upstream change
sc-lint is asked to stop flagging an awaited `tokio::time::sleep` and `sleep_until` under `SCB-RUNTIME-005` (issue linked from this ADR's PR). When a released sc-lint no longer flags them, the D3 allowances are removed in one change, and this ADR is amended to record that.

### D5. Tests are unchanged
This ADR changes nothing for tests. ADR-008 and REQ-P-TEST-001 still apply. Tests use a paused clock with `advance`, in-memory transports and controlled inputs, and never a real sleep.

## Consequences

- The legitimate production timers keep their allowances, now with ADR-070 as the reason. Their cancellation is fixed where it is missing (phase-bg PD-1, PD-9).
- Sleeps that stand in for a missing event are still fixed by wiring the event (phase-bg PD-8, the mailbox long-poll).
- Under a paused clock, tests control every tokio timer, so these sites remain deterministic to test.

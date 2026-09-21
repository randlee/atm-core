# BC.2 retained mappings and reduction

This layer migrates ATM's two observability adapters from the deprecated
`sc-observability` 1.4.0 compatibility errors to the stable 1.4.1 typed APIs.
The public ATM contracts remain unchanged.

## Removed production lines

Compared with the frozen BC.6 parent (`1a7c50a1a59afc3ff28edbc8acf5b4391440dd14`):

| Location | Removed/replaced production lines | Reason |
| --- | ---: | --- |
| `crates/atm-observability/src/lib.rs` | 23 removed, 16 added | Remove deprecated `Logger::builder`, `Logger::try_log`, and `Logger::flush` calls and their allowances; use `builder_typed/build_typed`, `try_log_typed`, and `flush_typed`. |
| `crates/atm/src/main.rs` | 15 removed, 9 added | Map typed `LogFailure`/`FlushFailure` while preserving the existing ATM error code/message contract. |
| **Net** | **38 removed, 25 added (13 fewer production lines)** | No public facade or JSON fixture change. |

## Intentionally retained mappings

- `RetainedLogOffer::{Accepted, QueueFull, Rejected}` remains ATM-owned. The
  non-blocking daemon admission contract must distinguish queue-full from other
  typed failures and must not block on a flush.
- `ObservabilityPort::emit` still performs a per-command `flush_typed()`. The
  CLI is a short-lived synchronous caller; daemon admission remains
  non-blocking and does not inherit this durability barrier.
- `AtmObservabilityHealth`, doctor state/detail, maintenance projection,
  `CommandEvent`, path checks, `ATM_LOG`, retained-field policy, and timeline
  types remain ATM-owned. Only backend diagnostic code extraction crosses the
  adapter boundary.
- The non-exhaustive typed failure matches retain a deterministic unknown-code
  fallback for forward-compatible upstream variants; no backend error is
  stringified to derive its code.
- The sealed adapter implementation and `pub mod sealed` visibility are
  unchanged; ADR-001's workspace-convention boundary remains in force.

## Equivalence evidence

- `sc-observability` and `sc-observability-types` are pinned to `=1.4.1` by
  BC.6.
- `TryLogFailure::QueueFull`, `LogFailure::{InvalidEvent, WriterDegraded,
  ShutdownTimedOut}`, and `FlushFailure` expose the same stable diagnostic
  codes consumed by the previous compatibility enums through
  `DiagnosticInfo::diagnostic()`.
- Focused tests passed: `cargo test -p atm-observability -p agent-team-mail`
  (`atm-observability`: 16 unit tests + 1 doctest; CLI: 312 unit tests and
  all integration suites).
- The BC.6 published-consumer qualification still passes, including health,
  queue-full, flush, shutdown, and query probes. Public ATM JSON/error/doctor
  fixtures remain byte-for-byte unchanged.

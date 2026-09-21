# bc.2 retained mappings and reduction

This layer migrates ATM's two observability adapters from the deprecated
`sc-observability` 1.4.0 compatibility errors to the stable 1.4.1 typed APIs.
The public ATM contracts remain unchanged.

## Removed production lines

Compared with the authoritative frozen parent (`0702bc285e1578e859754278ebb6dda2c87b0542`):

| Location | Removed/replaced production lines | Reason |
| --- | ---: | --- |
| `crates/atm-observability/src/lib.rs` | 0 removed, 0 added | Lower typed migration is inherited unchanged from the frozen parent. |
| `crates/atm/src/main.rs` | 27 removed, 108 added | Preserve governed ATM message/remediation byte shape while retaining upstream diagnostics in the internal `cause` channel and expanding golden mappings. |
| **Net** | **27 removed, 108 added (81 net lines)** | Public ATM error/message/remediation shape remains unchanged; only internal cause and tests expand. |

## Intentionally retained mappings

### Deprecated sink boundary inventory

The two `LogSink::{write,flush}` implementations in the CLI fault-injection
adapter deliberately retain upstream `LogSinkError` because sc-observability
1.4.1 exposes that compatibility trait as the only contract-compatible way to
wrap a sink and override health state. They are compiled only for `test` or
`fault-injection`; production admission and durability use the typed logger
facade (`log_typed`/`flush_typed`). The typed sink trait is not
contract-equivalent, so removing these allowances would either remove the
degraded/unavailable health seam or require an upstream API change.

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
  bc.6.
- `TryLogFailure::QueueFull`, `LogFailure::{InvalidEvent, WriterDegraded,
  ShutdownTimedOut}`, and `FlushFailure` expose the same stable diagnostic
  codes consumed by the previous compatibility enums through
  `DiagnosticInfo::diagnostic()`.
- Focused tests passed: `cargo test -p atm-observability -p agent-team-mail`
  (`atm-observability`: 17 unit tests + 1 doctest; CLI: 313 unit tests and
  all integration suites).
- The bc.6 published-consumer qualification still passes, including health,
  queue-full, flush, shutdown, and query probes. Public ATM JSON/error/doctor
  fixtures remain byte-for-byte unchanged.

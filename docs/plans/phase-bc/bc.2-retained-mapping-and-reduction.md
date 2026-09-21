# bc.2 retained mappings and reduction

This layer migrates ATM's two observability adapters from the deprecated
`sc-observability` 1.4.0 compatibility errors to the stable 1.4.1 typed APIs.
The public ATM contracts remain unchanged.

## Removed production lines

Compared with the authoritative immediate parent (`fe432fff14b659db4e12e5f0700244c2566eab43`):

| Location | Removed/replaced production lines | Reason |
| --- | ---: | --- |
| `crates/atm-observability/src/lib.rs` | 8 removed, 24 added | Extract typed diagnostics at the adapter boundary without changing the public error contract. |
| `crates/atm/src/main.rs` | 53 removed, 50 added | Preserve historical public messages while discarding backend diagnostics after stable typed extraction. |
| **Net** | **61 removed, 74 added (13 net lines)** | Public ATM error/message/remediation shape remains unchanged; test and telemetry plumbing are separated from the public contract. |

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
- Historical public error goldens remain anchored to the pre-BC2 baseline
  `df7a094e`; no live constructor output is used as the compatibility oracle.
- Focused tests passed: `cargo test -p atm-observability` (17 unit tests,
  2 macro tests, 1 doctest) and `cargo test -p agent-team-mail --bin atm`
  (314 unit tests). No backend paths outside the injected SQLite fault matrix
  are claimed as exercised.
- The bc.6 published-consumer qualification still passes, including health,
  queue-full, flush, shutdown, and query probes. Public ATM JSON/error/doctor
  fixtures remain byte-for-byte unchanged.

### bc.2 review-fix-5 receipt

This layer is based on immediate parent `fc4e1fff50b35b8d0e054808c774c4523591bb54`; historical public-contract goldens are frozen against `df7a094e`. Production coverage is limited to the CLI adapter and SQLite-backed retained logger. Other backend paths are unsupported by this fault matrix and are not claimed as tested. The production/test LOC split and CLI test count are recorded from the current tree, not inferred from a live constructor.

# bc.2 retained mappings and reduction

This layer migrates ATM's two observability adapters from the deprecated
`sc-observability` 1.4.0 compatibility errors to the stable 1.4.1 typed APIs.
The public ATM contracts remain unchanged.

## Removed production lines

The cumulative reduction is compared with the historical baseline
`fe432fff14b659db4e12e5f0700244c2566eab43`:

| Location | Removed | Added | Net | Reason |
| --- | ---: | ---: | ---: | --- |
| `crates/atm-observability/src/lib.rs` | 22 | 9 | -13 | Extract typed diagnostics at the adapter boundary without changing the public error contract. |
| `crates/atm/src/main.rs` | 40 | 21 | -19 | Preserve historical public messages while discarding backend diagnostics after stable typed extraction. |
| **Total** | **62** | **30** | **-32** | Public ATM error/message/remediation shape remains unchanged; test and telemetry plumbing are separated from the public contract. |

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
- Historical public error goldens remain anchored to the pre-bc.2 baseline
  `df7a094e07686f2a12878a548acecd7a4eea50e4`; no live constructor output is
  used as the compatibility oracle.
- Focused tests passed: `cargo test -p atm-observability` (17 unit tests,
  2 macro tests, 1 doctest) and `cargo test -p agent-team-mail --bin atm`
  (314 unit tests). No backend paths outside the retained JSONL sink health
  override are claimed as exercised.
- Literal matrix validation passed: `cargo test --locked -p agent-team-mail
  --bin atm adapter_tests::typed_failure_mappings_match_literal_historical_matrix`
  (1 passed, 313 filtered).
- The bc.6 published-consumer qualification still passes, including health,
  queue-full, flush, shutdown, and query probes. Public ATM JSON/error/doctor
  fixtures remain byte-for-byte unchanged.

### bc.2 review-fix-6 receipt

The preceding PR1562 layer has immediate parent
`fc4e1fff50b35b8d0e054808c774c4523591bb54` and exact head
`25794021a1b8a116cc73edaf751b485a7563b48f`; it changes `main.rs` by 31
additions and 17 deletions, split as production +18/-10 and tests +13/-7.
This correction branch has parent
`25794021a1b8a116cc73edaf751b485a7563b48f`; its correction implementation
commit exact head is `1ad222ba8b1ea984821eade64336abec8c70607a`. The
historical baseline is
`fe432fff14b659db4e12e5f0700244c2566eab43`.

The correction implementation changes `crates/atm/src/main.rs` by 84
additions and 61 deletions, split as production +8/-3 and tests +76/-58.
The production portion is the adapter mapping and unknown-code fallback; the
remaining portion is test-only literal coverage. The CLI test count is 314.

The exercised seam is a retained JSONL sink health override with successful
emit, flush, query, and terminal shutdown. It is not SQLite-backed and does
not inject real `InvalidEvent`, `WriterDegraded`, `ShutdownTimedOut`, query, or
follow backend failures; those paths are explicitly unsupported by this fault
matrix. This correction branch will be registered as the new draft top layer
of gh-stack #1550; PR1562 and PR1561 remain draft while these corrections
continue.

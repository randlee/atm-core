---
status: implemented; critical-remediation review active
---

# bc.2 retained mappings and reduction

This layer migrates ATM's two observability adapters from the deprecated
`sc-observability` 1.4.0 compatibility errors to the stable 1.4.1 typed APIs.
The public ATM contracts remain unchanged.

## Cumulative removed production lines

The historical mid-remediation comparison remains anchored to
`fe432fff14b659db4e12e5f0700244c2566eab43`:

| Location | Removed | Added | Net | Reason |
| --- | ---: | ---: | ---: | --- |
| `crates/atm-observability/src/lib.rs` | 22 | 9 | -13 | Extract typed diagnostics at the adapter boundary without changing the public error contract. |
| `crates/atm/src/main.rs` | 22 | 14 | -8 | Preserve historical public messages while discarding backend diagnostics after stable typed extraction. |
| **Total** | **44** | **23** | **-21** | Public ATM error/message/remediation shape remains unchanged; test and telemetry plumbing are separated from the public contract. |

The recomputed production/test split for the final code head
`ce6b38d4919e05f22e502e5ad8e4145dc3c128d5`, compared with develop
`904673995c529017ce673c7957efa77176df61e4`, is **+36 lines**: `crates/atm/src/main.rs`
944 -> 975 (**+31**) and `crates/atm-observability/src/lib.rs` 514 -> 519
(**+5**). Test-only literal-matrix, query, and fault-injection additions are
separately covered by the focused test receipts below and are not included in
this production-line figure. The `fe432fff1` table is a mid-remediation comparison, not the
production delta from develop. The earlier `df7a094e0` receipt is likewise a
historical **+4-line** intermediate observation. Deliverable 5 is therefore
closed as a documented retained-mapping review, not as a claim of net source
line reduction against develop.

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
  `df7a094e07686f2a12878a548acecd7a4eea50e4`; the unconditional
  `retained_logger_bootstrap_error_preserves_historical_json_without_cause`
  golden test compares serialized `AtmError` output directly. Stable `SC_*`
  diagnostic codes are internal diagnostic/timeline data; they are not a
  replacement public JSON contract.
- Focused tests passed: `cargo test -p atm-observability` (17 unit tests,
  2 macro tests, 1 doctest) and `cargo test -p agent-team-mail --bin atm`
  (314 unit tests). No backend paths outside the retained JSONL sink health
  override are claimed as exercised.
- Literal matrix validation passed: `cargo test --locked -p agent-team-mail
  --bin atm adapter_tests::typed_failure_mappings_match_literal_historical_matrix`
  (1 passed, 313 filtered).
- The bc.6 published-consumer qualification still passes, including health,
  queue-full, flush, shutdown, and query probes. Public ATM JSON/error/doctor
  fixtures remain byte-for-byte unchanged after `ce6b38d`; no selective caveat
  is intended.

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

The final receipt is pinned to code head `ce6b38d4919e05f22e502e5ad8e4145dc3c128d5`;
the draft-PR narrative above is historical and is not a current delivery
claim. `RetainedLogger::flush -> typed::FlushFailure` is a public API change
present at the already-published workspace version `1.6.0`. The release-from-
main decision (major bump or `publish = false`) remains explicitly open for
the release owner; this documentation layer neither decides nor releases it.

The exercised seam is a retained JSONL sink health override with successful
emit, flush, query, and terminal shutdown. It is not SQLite-backed and does
not inject real `InvalidEvent`, `WriterDegraded`, `ShutdownTimedOut`, query, or
follow backend failures; those paths are explicitly unsupported by this fault
matrix. This correction branch will be registered as the new draft top layer
of the earlier stack; that draft-stack status is historical and is not a
current delivery claim.

The intentional retained-field privacy boundary remains: `CommandEvent` may
carry team, agent, sender, message-id, and task-id context, but retained JSON
is restricted to `RETAINED_FIELD_ALLOWLIST`; this phase adds no identity field
or synthesized correlation id. `prepare_retained_log` remains synchronous
because current CLI startup is single-task and no behaviour-preserving bounded
`spawn_blocking` reuse exists; future multi-task reuse requires review.

The sc-observability follow-ups [#203](https://github.com/randlee/sc-observability/issues/203)
and [#204](https://github.com/randlee/sc-observability/issues/204) remain
deferred upstream items.

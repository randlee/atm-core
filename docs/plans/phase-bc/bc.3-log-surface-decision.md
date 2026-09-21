# bc.3 log-surface decision

Evaluated against `sc-observability-log = 1.4.1` and the frozen ATM retained
contracts. The qualification fixture is isolated under
`crates/atm-observability/tests/macro_qualification.rs`; it installs one
temporary bridge in its own test process and never installs a second production
global logger.

## Disposition

| Surface | Decision | Evidence / boundary |
| --- | --- | --- |
| `event!`, `debug!`, `info!`, `warn!`, `error!`, `trace!` re-exports | Retain ATM ownership; isolated qualification only | Supported grammar compiles and emits through the temporary bridge. Production adoption requires an upstream interoperability capability that composes with ATM's single global owner while preserving ATM allowlist/redaction/timeline policy; none is present in 1.4.1. |
| `#[sc_observability_log::instrument]` synchronous functions | Retain as qualified option | Fixture proves success, error, panic completion, bounded fields, and explicit absence of correlation propagation. Adoption requires an explicit ATM field-policy adapter at each call site. |
| `#[instrument]` async functions | Retain as qualified option | Executor-agnostic expansion; fixture proves success, async error, cancellation/drop completion. It does not own Tokio/Axum or daemon lifecycle. |
| macro fields/arguments | Upstream capability required before ATM adoption | Macro field capture is useful but cannot bypass ATM's bounded payload, sensitive-field allowlist, or diagnostic timeline. |
| global `init` / `LogGuard` lifecycle | Retain ATM ownership | Upstream installs a process-global logger and owns shutdown. ATM production already has one global owner and adapter-specific shutdown authority. |
| counters and dropped-event accounting | Retain ATM ownership | Upstream counters are process-global and classify macro drops, while ATM's retained timeline/counter contract is richer and must remain the source of truth. |
| `#[instrument]` runtime semantics | Retain ATM ownership; isolated qualification only | Semantics are executor-agnostic, but production adoption requires the same absent upstream interoperability capability; no invented `sc-observability-tokio` dependency is needed. |

## Parity limits

The fixture establishes macro syntax, structured output, bounded admission,
target/cardinality/redaction checks, explicit correlation absence,
error/panic/cancellation completion, and compile-fail grammar. It
does not prove parity with ATM's retained target allowlist, redaction policy,
diagnostic timeline, or global install/shutdown ownership. Therefore no
production bridge replacement or call-site migration is justified in bc.3.

Validation includes the supported/rejected trybuild cases, async error and
cancellation, panic completion, JSONL cardinality/target/redaction comparison,
bounded flush/shutdown, and normal workspace gates.

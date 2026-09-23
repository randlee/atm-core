---
status: implemented; critical-remediation review active
---

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
| `#[sc_observability_log::instrument]` synchronous functions | Retain as qualified option | Fixture proves success, error, panic completion, and explicit absence of correlation propagation. Adoption requires an explicit ATM field-policy adapter at each call site. |
| `#[instrument]` async functions | Retain as qualified option | Executor-agnostic expansion; fixture proves success, async error, cancellation/drop completion. It does not own Tokio/Axum or daemon lifecycle. |
| macro fields/arguments | Upstream capability required before ATM adoption | Macro field capture is useful but cannot bypass ATM's sensitive-field allowlist or diagnostic timeline. ATM's production policy is a key allowlist; no bounded-payload guarantee is claimed for the upstream fixture. |
| global `init` / `LogGuard` lifecycle | Retain ATM ownership | Upstream installs a process-global logger and owns shutdown. ATM production already has one global owner and adapter-specific shutdown authority. |
| counters and dropped-event accounting | Retain ATM ownership | Upstream counters are process-global and classify macro drops, while ATM's retained timeline/counter contract is richer and must remain the source of truth. |
| `#[instrument]` runtime semantics | Retain ATM ownership; isolated qualification only | Semantics are executor-agnostic, but production adoption requires the same absent upstream interoperability capability; no invented `sc-observability-tokio` dependency is needed. |

## Parity limits

The fixture establishes macro syntax, structured output, explicit
denylisted-value redaction, and the upstream retained outcomes for an
oversized value and an event with excess fields. Those oversized/excess
values are intentionally retained by the upstream fixture; that is evidence
of upstream behavior, not ATM parity. The supported ATM boundary is a key
allowlist, not a bounded-payload claim for the upstream macro. Unsupported
parity gaps remain ATM's diagnostic timeline and global install/shutdown
ownership. The fixture also establishes
explicit correlation absence, error/panic/cancellation completion, and
compile-fail grammar. Therefore no production bridge replacement or call-site
migration is justified in bc.3.

The fixture is an upstream-behaviour capture: receipt `17` positive/negative
trybuild cases + `2` async completion cases + `1` macro output comparison
(`17+2+1`). The upstream follow-ups sc-observability issues [#203](https://github.com/randlee/sc-observability/issues/203)
and [#204](https://github.com/randlee/sc-observability/issues/204) are
deferred and are cited here rather than claimed as ATM fixes.

Validation includes the supported/rejected trybuild cases, async error and
cancellation, panic completion, JSONL cardinality/target/redaction comparison,
bounded flush/shutdown, and normal workspace gates.

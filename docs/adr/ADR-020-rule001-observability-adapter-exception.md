# ADR-020 — RULE-001 Observability Adapter Exception

| Field | Value |
|---|---|
| ID | ADR-020 |
| Status | **Accepted** |
| Date | 2026-07-08 |
| Deciders | Rand Lee |
| Relates to | ADR-001, ADR-018, ADR-019, AD18/ARCH-004 |
| Supersedes | — |

---

> **Amendment (2026-09-05, docs-only, team-lead):** the `atm-daemon` crate
> was split and the daemon runtime moved to `atm-daemon-bootstrap`; the
> sanctioned adapter module was carried forward as
> `crates/atm-daemon-bootstrap/src/daemon_observability.rs`. Path references
> below are updated to the current location. No policy content changed —
> `.just/allowlists/scb_observability_allowlist.toml` (rule
> `SCB-OBSERVABILITY-001`) already enforces the exception at this current
> path, and a repo-wide grep confirms it remains the only non-`main.rs`
> daemon source importing `sc_observability_types` directly.

---

## Context

`RULE-001` forbids direct `sc_observability_types` imports from arbitrary
daemon library modules. During the `AD.25` through `AD.30` follow-up planning
review, arch-qa re-verified the actual `atm-daemon` module graph and found:

- `crates/atm-daemon-bootstrap/src/daemon_observability.rs` is declared via
  `mod daemon_observability;` in `crates/atm-daemon-bootstrap/src/lib.rs`
- `lib.rs` publicly re-exports daemon observability surface items from that
  module
- therefore the file is a real library module, not a binary-internal file
- at the time of this decision, `runtime_sqlite_observer.rs` and
  `test_observability.rs` needed access to the same `ActionName` /
  `OutcomeLabel` types to call `DaemonRuntimeObservability` (those specific
  consumer files have since been reorganized; the adapter module itself is
  the load-bearing exception surface, not the specific consumer file names)

The earlier "binary-internal seam" framing was factually wrong against the
source tree and could not support a truthful exception.

At the same time, forcing every construction site up into `main.rs` would widen
the daemon entrypoint's ownership beyond what this follow-up line needs to
close. The accepted near-term fix is therefore a narrowly-scoped
library-internal adapter exception with explicit lint enforcement and review
conditions.

## Decision

Accept one sanctioned library-internal adapter exception to `RULE-001` for the
`atm-daemon` crate:

- `crates/atm-daemon-bootstrap/src/daemon_observability.rs` is the only
  sanctioned non-`main.rs` daemon source file allowed to import
  `sc_observability_types::{ActionName, OutcomeLabel}` directly
- that adapter module must expose a concrete achievable crate-visible
  mechanism, such as:
  - `pub(crate)` aliases
  - `pub(crate)` constructor helpers
  - another equally narrow crate-visible adapter surface
- every other daemon-internal consumer must consume that crate-visible
  adapter surface instead of importing `sc_observability_types` directly
- no other file under `crates/atm-daemon-bootstrap/src/` may add a new direct import of
  `ActionName` or `OutcomeLabel`

## Enforcement

This exception is valid only with mechanical enforcement:

- `AD.26` must wire `.just/lint_boundaries.py` to reject direct
  `sc_observability_types::{ActionName, OutcomeLabel}` imports anywhere under
  `crates/atm-daemon-bootstrap/src/` except the sanctioned adapter module and `main.rs`
- the lint rule must use the repository's existing TOML allowlist pattern
  rather than a one-off hard-coded path check
- because the sanctioned import lives at module root, the allowlist mechanism
  must support one explicit module-root sentinel such as
  `symbol = "__module__"`
- a known-bad fixture must prove the lint fails when a new direct import is
  introduced elsewhere in the daemon tree

## Boundary Conditions

This ADR does not relax any other daemon boundary:

- it does not authorize direct SQLite access
- it does not authorize direct post-send policy selection in the adapter module
- it does not authorize new crate-wide re-exports of third-party observability
  types as public API
- it does not convert `atm-daemon` into a general-purpose observability facade

The exception exists only to keep one narrow third-party type dependency inside
one sanctioned library-internal adapter module while the rest of the daemon
depends on crate-local aliases/helpers.

## Consequences

### Positive

- the accepted plan now matches the actual `lib.rs` module graph truthfully
- the `RULE-001` exception becomes implementable for `runtime_sqlite_observer`
  and `test_observability`
- CI can catch future drift rather than depending on plan-QA grep review

### Negative

- `RULE-001` is no longer absolute for the daemon crate
- the lint framework must grow one more allowlist-backed exception family
- the adapter module becomes a consciously-governed dependency concentrator

## Review Conditions

The exception remains acceptable only while all of the following stay true:

- the direct imports remain confined to
  `crates/atm-daemon-bootstrap/src/daemon_observability.rs`
- downstream daemon modules consume only the sanctioned crate-visible adapter
  surface
- `.just/lint_boundaries.py` and its fixture/allowlist keep enforcing the
  boundary
- no public API outside the crate starts requiring consumers to name
  `sc_observability_types::ActionName` or `OutcomeLabel` directly

If any of those conditions stop being true, this ADR must be reopened rather
than silently widening the exception.

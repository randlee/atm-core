# sprint bc.3 — qualify log macros and the instrument attribute

Accountable owner: `arch-ctm@atm-dev`.

## goal

Determine, with executable parity evidence, which `sc-observability-log`
features can replace ATM code safely. Do not force production migration where
upstream policy differs.

## deliverables

1. Add an isolated consumer fixture for the re-exported event macros and
   `#[sc_observability_log::instrument]` on synchronous and async functions.
2. Cover success, error, panic, and async cancellation/drop completion.
3. Compare upstream output with ATM's retained target, allowlist/redaction,
   correlation, diagnostic-timeline, counter, install, and shutdown contracts.
4. Write an architecture decision with a closed disposition for each surface:
   adopt now, retain ATM implementation, or require an upstream capability.
5. If and only if parity is already proven, migrate one bounded non-critical
   call path and measure net code reduction. Otherwise the executable proof and
   explicit non-adoption decision are the production-ready deliverables.

## acceptance

- The fixture proves macro expansion/runtime behavior without installing a
  second global logger in ATM production processes.
- `#[instrument]` is described as executor-agnostic; no Tokio-specific crate or
  runtime ownership is asserted.
- No duplicate event, timeline bypass, lost correlation, or
  changed shutdown authority is accepted.
- Any future global bridge migration has explicit prerequisites and is not
  hidden in this sprint.
- `docs/plans/phase-bc/bc.3-log-surface-decision.md` records an evidence-backed
  adopt, retain, or upstream-capability disposition for every evaluated
  surface.

## required validation

Run trybuild/compile-fail coverage for supported and rejected macro grammar,
async cancellation/panic tests, retained-output comparison, and the normal
phase gates.

## out of scope

Wholesale bridge replacement, a second global logger, changes to ATM retained
field/redaction policy, a Tokio-specific integration crate, or production
migration without parity evidence.

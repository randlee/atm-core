# sprint bc.2 — consolidate typed observability mappings

Accountable owner: `arch-ctm@atm-dev`.

## goal

Reduce duplicated backend error and health mapping while keeping ATM's sealed
facade and observable behavior unchanged.

## deliverables

1. Adopt stable `1.4.0` typed error/diagnostic APIs in
   `crates/atm-observability/src/lib.rs` and the CLI adapter.
2. Remove redundant variant matching or primitive validation only where the
   upstream stable code/remediation/newtype contract is equivalent.
3. Keep `ObservabilityPort`, `CommandEvent`, ATM doctor types, path checks,
   `ATM_LOG`, and diagnostic-timeline types ATM-owned.
4. Add golden tests for stable ATM error code, remediation, health detail,
   queue-full, stopped, degraded writer, query, flush, and shutdown outcomes.
5. Record net production-line reduction and any intentionally retained mapping.

## acceptance

- No backend error is collapsed to an opaque string before ATM extracts its
  stable code and recovery context (RBP-001).
- No raw-string wrapper duplicates a validated upstream newtype (RBP-004).
- CLI per-command flush and daemon non-blocking admission remain distinct.
- Public ATM JSON/error/doctor fixtures are unchanged unless the sprint doc is
  amended with an approved governed-interface change.
- The diff does not modify `pub mod sealed` or add an unauthorized
  `sealed::Sealed` implementation.
- `docs/plans/phase-bc/bc.2-retained-mapping-and-reduction.md` records removed
  production lines, each intentionally retained mapping, and the equivalence
  evidence for every removed mapping.

## required validation

Run the full phase gate plus focused observability, CLI log/query/follow,
daemon shutdown, fault-injection, and boundary-enforcement suites.

## out of scope

Changing ATM public error/doctor contracts, changing sealed-trait ownership,
replacing the tracing bridge, adopting log macros, or changing release logic.

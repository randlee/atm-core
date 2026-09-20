# sprint bc.6 — adopt the coordinated sc-observability 1.4.1 republish

Accountable owner: `arch-ctm@atm-dev`.

## goal

Advance the qualified `1.4.0` work to exact `1.4.1` pins after the complete
coordinated release exists and is independently verified.

## entry gate

All required sc-observability crates are published at `1.4.1`, their packages
and release evidence are independently verified, the dependency graph is
internally lockstep, and the upstream owner has supplied the intended consumer
inventory. Upstream preparation is tracked by sc-observability
[PR #197](https://github.com/randlee/sc-observability/pull/197), with
`cobs@sc-obs` owning version/manifests/validation; `aobs@sc-obs` supplies the
published/verified completion notice.

The release uses sc-observability's existing installed shared workflow. This
entry gate does not require sc-publish PR #101, bc.4, or any phase-bc
immutable-release gate.

## deliverables

1. Update every direct phase-owned sc-observability workspace dependency from
   exact `=1.4.0` to exact `=1.4.1`; update `Cargo.lock`.
2. Update `sc-observability-log` and its macro implementation only as the
   published public facade directs; do not depend directly on the proc-macro
   implementation unless upstream explicitly requires it.
3. Re-run the bc.1 consumer and behavior qualification. If bc.2 or bc.3 is
   already frozen when this layer starts, also re-run its focused suites.
4. Record published package identities, API diff, publication verification,
   and validation receipts in
   `docs/plans/phase-bc/bc.6-1.4.1-requalification.md`.

## acceptance

- No nonexistent or partially published `1.4.1` version is committed.
- All resolved sc-observability family versions are the intended coordinated
  set with the log/macro lockstep preserved.
- The published `1.4.0..1.4.1` source and API diff is reviewed; no assumption
  of identical source or API substitutes for that review.
- ATM public behavior and all parity fixtures remain satisfied.
- The change is the priority append-next layer above the frozen top when `u5`
  closes; frozen `1.4.0` qualification history is not rewritten.
- No sc-publish PR #101 adoption or phase-bc immutable-release gate is treated
  as a prerequisite for upstream preparation or ATM consumption.
- The requalification artifact closes `u5` against the exact packages consumed
  by the lockfile.

## required validation

Run the full phase gates on the exact lockfile, cross-platform consumer builds,
published-package/source checks, API compatibility diff, observability fault
matrix, macro fixture, and upstream publication-verification receipts.

## out of scope

Rewriting the frozen `1.4.0` qualification layer, accepting a partial family
publish, assuming source/API identity without review, or migrating unrelated
observability surfaces.

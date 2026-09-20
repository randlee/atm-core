# sprint bc.6 — adopt the coordinated sc-observability 1.4.1 republish

Accountable owner: `arch-ctm@atm-dev`.

## goal

Advance the qualified `1.4.0` work to exact `1.4.1` pins after the complete
coordinated release exists and is independently verified.

## entry gate

All required sc-observability crates are published at `1.4.1`, their release
tag/assets are immutable and verified, the dependency graph is internally
lockstep, and the upstream owner has supplied the intended consumer inventory.

## deliverables

1. Update every direct phase-owned sc-observability workspace dependency from
   exact `=1.4.0` to exact `=1.4.1`; update `Cargo.lock`.
2. Update `sc-observability-log` and its macro implementation only as the
   published public facade directs; do not depend directly on the proc-macro
   implementation unless upstream explicitly requires it.
3. Re-run the bc.1 consumer and behavior qualification plus bc.2/bc.3 focused
   suites.
4. Record published package identities, API diff, immutable release proof, and
   validation receipts in
   `docs/plans/phase-bc/bc.6-1.4.1-requalification.md`.

## acceptance

- No nonexistent or partially published `1.4.1` version is committed.
- All resolved sc-observability family versions are the intended coordinated
  set with the log/macro lockstep preserved.
- The published `1.4.0..1.4.1` source and API diff is reviewed; no assumption
  of identical source or API substitutes for that review.
- ATM public behavior and all parity fixtures remain satisfied.
- The change is an append-only top layer; frozen `1.4.0` qualification history
  is not rewritten.
- The requalification artifact closes `u5` against the exact packages consumed
  by the lockfile.

## required validation

Run the full phase gates on the exact lockfile, cross-platform consumer builds,
published-package/source checks, API compatibility diff, observability fault
matrix, macro fixture, and immutable release verification.

## out of scope

Rewriting the frozen `1.4.0` qualification layer, accepting a partial family
publish, assuming source/API identity without review, or migrating unrelated
observability surfaces.

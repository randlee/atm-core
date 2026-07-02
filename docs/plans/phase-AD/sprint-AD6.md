# AD.6 Proc-Macro Registry Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.6
worktree: ../atm-core-worktrees/feature/pAD-s6-proc-macro-registry-cutover
branch: feature/pAD-s6-proc-macro-registry-cutover
status: planned
estimated_scope: medium
```

## Goal

Replace ATM's vendored `sc-lint-attributes` compile-time dependency with the
published crate line and prove the exact ATM `#[sc_lint(...)]` usage remains
compile-valid and semantically acceptable.

## Scope Summary

This sprint closes only the proc-macro registry cutover.

Production-ready commitment:
- the proc-macro dependency must come from the published registry line rather
  than an ATM-vendored path dependency
- whole-workspace success alone is not sufficient proof; the real ATM
  attribute call sites must be exercised explicitly

Required dependency and call-site shape:

```toml
[dependencies]
sc-lint-attributes = "<published version>"
```

```rust
#[sc_lint(boundary.allow("cycle.recursive_value_container"))]
```

## Prerequisites

- `AD.5`

## Out Of Scope

- no vendored crate removal yet
- no CI / release-preflight cutover yet
- no dependency-policy Phase `D.1` adoption yet

## Code And Document Targets

- `crates/atm-core/Cargo.toml`
- `crates/atm-core/src/observability.rs`
- any repo-owned version pin or install-contract record introduced earlier in
  the phase

## Deliverables

- `crates/atm-core/Cargo.toml` no longer path-depends on vendored
  `sc-lint-attributes`
- any required registry pinning for the published proc-macro surface is
  reviewable in repo state
- the exact `#[sc_lint(...)]` attribute call sites in
  `crates/atm-core/src/observability.rs` are compiled and validated under the
  published proc-macro dependency

## Required Work

- replace the vendored proc-macro dependency with the published registry line
- keep ATM source changes limited to what the proc-macro cutover strictly
  requires
- exercise the real `observability.rs` attribute usage explicitly, not only a
  whole-workspace build
- block closure if the published directive grammar requires unplanned source
  redesign

## Acceptance Criteria

- ATM compiles with the published proc-macro dependency instead of the vendored
  path dependency
- the current `#[sc_lint(...)]` directive grammar used by ATM remains valid
- any source change beyond what is required for the proc-macro cutover is
  treated as out of scope and blocks sprint closure

## Required Validation

- `cargo build --workspace`
- `cargo test -p atm-core observability`
- `! rg -n 'path = \"\\.\\./sc-lint-attributes\"|path = \"\\.\\./sc-lint-directives\"' Cargo.toml crates/*/Cargo.toml -S`
- `git diff --check`

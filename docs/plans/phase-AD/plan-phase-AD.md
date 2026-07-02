---
title: Phase AD Plan
status: proposed
branch: plan/sc-lint-published-migration
worktree: ../atm-core-worktrees/plan/sc-lint-published-migration
---

# Phase AD Plan

## Goal

Migrate `atm-core` off its vendored `sc-lint` workspace crates and onto the
published `sc-lint` release line without weakening:

- proc-macro compatibility
- repo-local lint wrappers
- CI lint behavior
- release-preflight behavior
- boundary-policy enforcement

Phase `AD` also owns duplicate-surface cleanup. The end state is not merely
"ATM can use published `sc-lint`." The end state is:

- published `sc-lint` binaries and crates are the only load-bearing
  implementation for the analyzer and proc-macro surfaces ATM consumes
- ATM keeps only repo-specific wrappers, formatting, and boundary-governance
  checks that are intentionally ATM-owned
- repo-local duplicate implementation is deleted once published parity is
  proven

## Authorization And Current Status

This document is a proposed execution package, not an approved implementation
line yet.

The original scoped deliverable on this planning branch was the supporting
inventory and gap analysis in
[`docs/plans/sc-lint-migration/plan.md`](../sc-lint-migration/plan.md). After
that inventory pass, the user asked for a phase-structured migration plan, so
this branch added the `Phase AD` phase doc and sprint set as a planning
proposal.

That proposal does not authorize execution by itself. Until a human reviewer
explicitly signs off on `Phase AD`, all branch/worktree names below are
reserved planning targets only:

- `plan/sc-lint-published-migration` is the planning branch
- `integrate/phase-AD` is the proposed execution integration branch
- `feature/pAD-s*-...` names are proposed sprint branches, not active coding
  assignments

## Scope Summary

`atm-core` currently vendors an older integrated `sc-lint` snapshot while the
standalone `sc-lint` repo has already split the product surface across:

- `sc-lint`
- `sc-lint-boundary`
- `sc-lint-portability`
- `sc-lint-runtime`
- `sc-lint-schema`
- `sc-lint-attributes`
- `sc-lint-directives`

The current ATM Python lint entrypoints fall into two buckets:

- thin ATM-owned adapters that should remain:
  - `.just/run_lint.py`
  - `.just/lint_unix_gating.py`
  - `.just/lint_runtime_waits.py`
  - `.just/lint_boundaries.py` for ATM-specific boundary-TOML schema,
    allowlists, and review gates
- implementation surfaces that should move fully onto published `sc-lint`:
  - `.just/lint_sc_boundary.py`
  - `.just/lint_sc_portability.py`
  - the proc-macro dependency in `crates/atm-core/Cargo.toml`
  - vendored crates under `crates/sc-lint-*`

That means the migration is not one closure. It must close the install,
analyzer, wrapper-subset, proc-macro, deletion, CI, and dependency-policy
ownership lines independently.

## Supporting Inventory

The detailed current-state inventory and gap analysis remain in:

- [`docs/plans/sc-lint-migration/plan.md`](../sc-lint-migration/plan.md)

That document remains the original inventory deliverable. The proposed
execution planning surface for this work is Phase `AD` plus the sprint docs
below, pending explicit human sign-off.

## Governing Documents

Phase `AD` planning and review must read these documents directly from disk:

- product requirements and architecture:
  - [`docs/requirements.md`](../../requirements.md)
  - [`docs/architecture.md`](../../architecture.md)
- affected crate docs:
  - [`docs/atm-core/requirements.md`](../../atm-core/requirements.md)
  - [`docs/atm-core/architecture.md`](../../atm-core/architecture.md)
  - [`docs/atm-core/boundaries.md`](../../atm-core/boundaries.md)
  - [`docs/atm/requirements.md`](../../atm/requirements.md)
  - [`docs/atm/architecture.md`](../../atm/architecture.md)
  - [`docs/atm/boundaries.md`](../../atm/boundaries.md)
  - during `AD.9`, also read the owning crate-local `requirements.md`,
    `architecture.md`, and `boundaries.md` docs for every boundary record whose
    dependency-policy semantics are touched by released `D.1` enforcement
- `sc-lint` product and boundary-model docs retained in this repo:
  - [`docs/sc-lint/requirements.md`](../../sc-lint/requirements.md)
  - [`docs/sc-lint/README.md`](../../sc-lint/README.md)
  - [`docs/sc-lint/boundary-enforcement-model.md`](../../sc-lint/boundary-enforcement-model.md)
  - [`docs/sc-lint/adr/ADR-004-structured-boundary-definitions.md`](../../sc-lint/adr/ADR-004-structured-boundary-definitions.md)
- ADR index and relevant architectural policy:
  - [`docs/adr/INDEX.md`](../../adr/INDEX.md)
  - [`docs/adr/ADR-007-supported-platform-parity.md`](../../adr/ADR-007-supported-platform-parity.md)
- machine-readable boundaries and boundary-governance surfaces:
  - current `boundaries/**/*.toml` records
  - `.just/lint_boundaries.py`
- testing and cross-platform guidance:
  - [`docs/testing-guidelines.md`](../../testing-guidelines.md)
  - [`docs/cross-platform-guidelines.md`](../../cross-platform-guidelines.md)
- process and coordination guidance:
  - [`docs/team-protocol.md`](../../team-protocol.md)
- phase-local issues inventory:
  - [`docs/plans/phase-AD/issues.md`](./issues.md)

## Phase Rules

Phase `AD` must preserve these invariants:

- ATM keeps its repo-local lint target names:
  - `sc-boundary`
  - `sc-portability`
  - `unix-gating`
  - `runtime-waits`
- ATM may retarget backend commands, but must not silently redesign the local
  lint UX during the same sprint.
- No sprint may require CI or local validation to shell through `../sc-lint`.
  Only published release artifacts or registry-published crates are allowed.
- No sprint may claim closure for more than one of these concerns:
  - published tool install contract
  - `sc-boundary` wrapper cutover
  - `sc-portability` wrapper cutover
  - `unix-gating` wrapper cutover
  - `runtime-waits` wrapper cutover
  - proc-macro registry cutover
  - vendored crate removal
  - CI / release-preflight cutover
  - dependency-policy ownership cutover
- Proc-macro compatibility is not considered proven by a generic workspace
  build alone; the exact `#[sc_lint(...)]` usage in
  `crates/atm-core/src/observability.rs` must be exercised explicitly.
- Published `sc-lint` capability adoption is the default. ATM keeps a local
  Python wrapper only when it is:
  - a repo-specific lint subset
  - repo-specific report formatting
  - repo-specific orchestration
  - an ATM-only boundary-governance rule not provided by released `sc-lint`
- `AD.4` and `AD.5` preserve ATM-owned subset semantics, not blind published
  rule-id continuity. If the released analyzer keeps equivalent semantics but
  renames the emitted rule IDs, the sprint must record one explicit upstream-
  to-ATM rule mapping artifact and keep the ATM wrapper contract intentional.
- `lint_boundaries.py` may keep ATM-only boundary schema and review-gate logic,
  but any dependency-policy checks that released `sc-lint` `D.1` covers at
  equal or better strength must move to the published analyzer and be deleted
  or reduced in ATM during `AD.9`.
- if `AD.9` changes dependency-policy semantics in any `boundaries/**/*.toml`
  record, the owning crate-local requirements, architecture, and boundary docs
  for those records must be updated in the same sprint rather than left in
  contradiction with the enforced TOML state
- `AD.4` owns closure of the unresolved `unix_path_prefixes` portability-config
  gap identified in the supporting inventory. The sprint must prove whether the
  published `sc-lint-portability` surface preserves that knob directly or
  whether ATM must carry the behavior forward in a documented wrapper-owned
  override.
- The full phase target is adoption of a released `sc-lint` version containing
  Phase `D.1` dependency-policy enforcement plus revalidation against ATM's
  boundary inventory, but external-release lag may leave `AD.9` as the only
  open follow-on sprint after `AD.8`.

## Baseline

- planning branch:
  - `plan/sc-lint-published-migration`
- proposed execution integration branch:
  - `integrate/phase-AD`
- current ATM vendored crates:
  - `crates/sc-lint-directives`
  - `crates/sc-lint-attributes`
  - `crates/sc-lint-boundary`
- current compile-time consumer:
  - `crates/atm-core/Cargo.toml` path-depends on `sc-lint-attributes`
- current wrapper model:
  - `.just/lint_sc_boundary.py`
  - `.just/lint_sc_portability.py`
  - `.just/lint_unix_gating.py`
  - `.just/lint_runtime_waits.py`
- current release-line proxy for the published tool family:
  - sibling `../sc-lint`

## Phase Entry Criteria

Phase `AD` execution begins only when:

- explicit human sign-off has approved the proposed `Phase AD` execution line
  described in `Authorization And Current Status`
- a published `sc-lint` release exists for the tool and crate surfaces ATM
  intends to consume first
- ATM has one supported installation path for those published tools on:
  - Linux
  - macOS
  - Windows
- ATM can validate that install path without relying on a sibling checkout

Phase `AD` may begin before released `Phase D.1` support exists, but the phase
must remain open until `AD.9` is complete.

## External Dependency Checkpoint Policy

`AD.9` depends on a released upstream `sc-lint` `D.1` surface that ATM does
not control. To avoid an unbounded silent stall:

- `AD.1` through `AD.8` may be declared functionally complete once their local
  cutover, deletion, CI, and release-preflight gates are green
- if `AD.8` closes before a released `D.1` exists, `Phase AD` remains open
  with `AD.9` as the only blocked follow-on sprint
- at each ATM release-planning checkpoint after `AD.8`, `team-lead` must
  re-review the upstream published state and record one explicit outcome in
  `.triage/phase-AD/ad9-checkpoint-log.md`
- checkpoint records must carry an explicit cycle number:
  - `Checkpoint cycle: 1/2`
  - or `Checkpoint cycle: 2/2`
- on checkpoint cycle `1/2`, the allowed outcomes are:
  - keep `AD.9` open against one new checkpoint date
  - or, with explicit human sign-off, spin `AD.9` into a standalone follow-on
    phase if the upstream release slips or materially changes scope
- checkpoint cycle `2/2` is the hard cap for this phase plan:
  - `keep AD.9 open` is no longer an allowed default outcome
  - explicit human re-scoping must choose either a standalone follow-on phase,
    an approved replacement plan against the real upstream state, or an
    explicit decision to stop carrying the unresolved dependency inside Phase
    `AD`
- silent indefinite carry-forward of `AD.9` is not allowed, and repeating
  checkpoint cycles beyond `2/2` is not allowed

## Phase Issues Inventory

The current open planning and execution risks for this phase are tracked in:

- [`docs/plans/phase-AD/issues.md`](./issues.md)

## Duplicate-Surface Classification

These surfaces are expected to remain ATM-owned after Phase `AD`:

- `.just/run_lint.py`
- `.just/lint_sc_boundary.py`
- `.just/lint_sc_portability.py`
- `.just/lint_unix_gating.py`
- `.just/lint_runtime_waits.py`
- the ATM-only portions of `.just/lint_boundaries.py`
- wrapper contract tests that protect ATM target names and report shape

These surfaces are expected to be deleted or cease to be load-bearing:

- `crates/sc-lint-directives`
- `crates/sc-lint-attributes`
- `crates/sc-lint-boundary`
- any wrapper/backend code path that still shells through `cargo run -p
  sc-lint-*`
- any ATM-owned dependency-policy enforcement inside `lint_boundaries.py` that
  is proven redundant by released `sc-lint` `D.1`

## Sprint Sequence

### AD.1 Published Tool Install Contract And Version Pin

Purpose:

- freeze one published `sc-lint` version pin
- land the repo-owned installation path ATM will use locally and in CI
- prove published analyzers are installable on all supported CI platforms
  without changing ATM wrapper behavior yet

Proposed execution branch:
- `feature/pAD-s1-published-tool-install-contract`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s1-published-tool-install-contract`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD1.md`](./sprint-AD1.md)

### AD.2 Boundary Wrapper Published Cutover

Purpose:

- retarget only `.just/lint_sc_boundary.py` to the published
  `sc-lint-boundary` binary
- prove full boundary-analyzer parity before any deletion or other wrapper
  retargeting starts

Proposed execution branch:
- `feature/pAD-s2-boundary-wrapper-published-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s2-boundary-wrapper-published-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD2.md`](./sprint-AD2.md)

### AD.3 Portability Wrapper Published Cutover

Purpose:

- retarget only `.just/lint_sc_portability.py` to the published
  `sc-lint-portability` binary
- prove portability parity before the consumer-subset wrappers move

Proposed execution branch:
- `feature/pAD-s3-portability-wrapper-published-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s3-portability-wrapper-published-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD3.md`](./sprint-AD3.md)

### AD.4 Unix-Gating Wrapper Published Cutover

Purpose:

- keep `unix-gating` as an ATM-owned subset wrapper while moving its backend to
  published portability findings only
- prove the repo-specific `PORT-004` / `PORT-005` contract survives intact,
  either through direct published rule-id continuity or through an explicit
  repo-owned mapping from published rule IDs to the ATM wrapper surface

Proposed execution branch:
- `feature/pAD-s4-unix-gating-wrapper-published-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s4-unix-gating-wrapper-published-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD4.md`](./sprint-AD4.md)

### AD.5 Runtime-Waits Wrapper Published Cutover

Purpose:

- keep `runtime-waits` as an ATM-owned subset wrapper while moving its backend
  to the published runtime analyzer
- prove the `SCB-RUNTIME-001` / `SCB-RUNTIME-002` contract no longer depends on
  the old integrated vendored boundary crate, either through direct published
  rule-id continuity or through an explicit repo-owned mapping from published
  rule IDs to the ATM wrapper surface

Proposed execution branch:
- `feature/pAD-s5-runtime-waits-wrapper-published-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s5-runtime-waits-wrapper-published-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD5.md`](./sprint-AD5.md)

### AD.6 Proc-Macro Registry Cutover

Purpose:

- replace the vendored proc-macro path dependency with the published crate line
- prove the exact ATM `#[sc_lint(...)]` usage remains compile-valid and
  semantically acceptable

Proposed execution branch:
- `feature/pAD-s6-proc-macro-registry-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s6-proc-macro-registry-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD6.md`](./sprint-AD6.md)

### AD.7 Vendored Crate Removal

Purpose:

- delete the vendored `sc-lint-*` workspace members after wrapper and
  proc-macro parity are already proven
- remove the now-stale duplicate implementation from the ATM repo

Proposed execution branch:
- `feature/pAD-s7-vendored-crate-removal`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s7-vendored-crate-removal`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD7.md`](./sprint-AD7.md)

### AD.8 CI And Release-Preflight Published Tool Cutover

Purpose:

- retarget CI and release-preflight to the published install path only
- prove ATM release gating no longer depends on workspace-built `sc-lint`

Proposed execution branch:
- `feature/pAD-s8-ci-release-published-tool-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s8-ci-release-published-tool-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD8.md`](./sprint-AD8.md)

### AD.9 Dependency-Policy Ownership Cutover On Released Phase D.1

Purpose:

- adopt the first released `sc-lint` version that includes Phase `D.1`
  dependency-policy enforcement
- rerun ATM boundary inventory against the new `dependencies` rule family
- remove or reduce ATM-local duplicate dependency-policy checks once published
  `sc-lint` proves equal-or-better coverage
- record the exact released `D.1` invocation, touched boundary records,
  residual ATM-only checks, and owning-doc reconciliation in repo-owned AD.9
  artifacts

Proposed execution branch:
- `feature/pAD-s9-dependency-policy-ownership-cutover`

Proposed execution worktree:
- `../atm-core-worktrees/feature/pAD-s9-dependency-policy-ownership-cutover`

Authoritative sprint doc:
- [`docs/plans/phase-AD/sprint-AD9.md`](./sprint-AD9.md)

## Phase Closeout Criteria

Phase `AD` is complete only when:

- ATM no longer vendors the old `sc-lint` workspace crates
- ATM wrappers, CI, and release-preflight use the published `sc-lint` install
  path only
- the published proc-macro surface is proven against ATM's real
  `observability.rs` attributes
- the first released `sc-lint` `D.1` dependency-enforcement surface is adopted
  and green on ATM
- any duplicate ATM dependency-policy checks that released `D.1` supersedes are
  deleted or explicitly reduced to ATM-only governance checks
- no required boundary inventory fix remains deferred after `D.1` adoption

If upstream `D.1` is still unreleased after `AD.8`, the phase remains open
under the checkpoint policy above rather than claiming silent closeout.

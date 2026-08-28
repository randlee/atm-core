---
id: SCLINT.10
title: Vendored Workspace Removal
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 10 — Vendored Workspace Removal

## Partial-Scope Exception — 2026-08-27 (Rand, recorded by fenix)

`crates/sc-lint-attributes/` was removed ahead of this sprint by PR #1068
under Rand's direct, narrow authorization, to clear a v1.4.4 release-preflight
blocker: `agent-team-mail-core` carried a path dependency on the local
`publish = false` crate, which `cargo publish` cannot satisfy. Rand's rulings,
verbatim:

> "local sc-lint-attributes crate?  we are not publishing this to crates.io."
> "there is sc-lint-attribute crate already published as part of the sc-lint
> repo/project."
> "if sc-lint-attribute is no longer used locally, we should remove it
> completely." / "so there is no ambiguity around implementation."
> "we are planning to move all sc-lint to use the published crates, but
> haven't gotten a gap to make these changes yet."

Scope of the exception: `sc-lint-attributes` only. Both consumers (`atm-core`,
`sc-lint-boundary`) now depend on the published `sc-lint-attributes = "0.5"`.
Proc-macro parity was proven in PR #1068 by compile plus the full lint gate
(the published macro accepts the existing `#[sc_lint(boundary.allow(...))]`
call sites and `sc-boundary` still honors them), `just test`, and a clean
`just validate`. `crates/sc-lint-directives/` and `crates/sc-lint-boundary/`
remain vendored; analyzer parity remains unproven and stays gated on this
sprint's Hard Dependencies. This sprint's remaining scope is the removal of
those two crates.

## Goal

Delete the vendored sc-lint crates from the ATM workspace once direct or
justified replacement surfaces are already proven.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-03.md`
- `docs/plans/sc-lint-migration/sprint-04.md`
- `docs/plans/sc-lint-migration/sprint-05.md`
- `docs/plans/sc-lint-migration/sprint-06.md`
- `docs/plans/sc-lint-migration/sprint-09.md`

## Exact Targets

- `Cargo.toml`
- `Cargo.lock`
- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`
- `docs/plans/sc-lint-migration/sprint-10.md`

## Deliverables

- the vendored workspace members are deleted:
  - `crates/sc-lint-directives/`
  - `crates/sc-lint-attributes/`
  - `crates/sc-lint-boundary/`
- the root workspace and lockfile no longer reference those vendored members
- any test, helper, or doc assumptions about those directories are removed

## Acceptance Criteria

- no ATM workspace path dependency points at a vendored `sc-lint-*` crate
- no deleted vendored crate directory is still referenced by `Cargo.toml`,
  scripts, tests, CI, or docs

## Paths To Delete

- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`

## Required Validation

- `test ! -d crates/sc-lint-directives`
- `test ! -d crates/sc-lint-attributes`
- `test ! -d crates/sc-lint-boundary`
- `! rg -n 'crates/sc-lint-directives|crates/sc-lint-attributes|crates/sc-lint-boundary|path *= *".*sc-lint-(directives|attributes|boundary)"' Cargo.toml Cargo.lock .just .github scripts`
- `cargo build --workspace`
- `git diff --check`

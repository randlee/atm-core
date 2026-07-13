---
id: SCLINT.09
title: Compile-Time Dependency Retarget
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: TBD - integrate/phase-<new-id>, not develop directly
---

# Sprint 09 — Compile-Time Dependency Retarget

## Goal

Retarget any legitimate compile-time `sc-lint-*` dependencies to the released
line, or document a zero-dependency outcome explicitly.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-02.md`

## Exact Targets

- `Cargo.toml`
- `Cargo.lock`
- `crates/atm-core/Cargo.toml`
- `crates/atm-core/src/**/*.rs`
- `docs/plans/sc-lint-migration/sprint-09.md`

## Deliverables

- the branch explicitly lands in one of these states:
  - zero compile-time `sc-lint-*` dependencies remain
  - only the specific released compile-time dependencies still required remain
- if any proc-macro surface is intentionally reintroduced, the exact call sites
  are named and validated

## Acceptance Criteria

- no fake "retargeted" compile-time dependency is claimed when no live call
  site exists
- no vendored path dependency survives in compile-time manifests
- any surviving released dependency has one concrete call-site justification

## Paths To Delete

- any vendored compile-time path dependency proven unnecessary

## Required Validation

- `rg -n "sc-lint" Cargo.toml crates/atm-core/Cargo.toml crates/atm-core/src || true`
- `cargo build --workspace`
- `git diff --check`

---
id: AE.2
title: Preflight Crate Metadata Validation
status: planned
branch: feature/pAE-s2-preflight-crate-metadata
worktree: ../atm-core-worktrees/feature/pAE-s2-preflight-crate-metadata
target: integrate/phase-AE
---

# Sprint AE.2 — Preflight Crate Metadata Validation

## Goal

- ensure every publishable crate has crates.io-required metadata before release

## Hard Dependencies

- `AE.1` complete
- `docs/plans/phase-AE/plan-phase-AE.md`
- `#435`

## Exact Targets

- `crates/atm/Cargo.toml`
- `crates/atm-core/Cargo.toml`
- `crates/atm-daemon/Cargo.toml`
- `crates/atm-runtime/Cargo.toml`
- `crates/atm-rusqlite/Cargo.toml`
- `crates/atm-storage/Cargo.toml`
- `crates/atm-graft/Cargo.toml`
- `.just/preflight.py` (new or extended)
- `release/publish-surface-scope.md`

## Required Metadata Per Crate

Every publishable crate must have:

```toml
[package]
description = "..."   # non-empty, crate-specific
license = "MIT"       # or "MIT OR Apache-2.0"
repository = "https://github.com/randlee/atm-core"
homepage = "https://github.com/randlee/atm-core"
documentation = "https://docs.rs/atm-core"  # or crate-specific
readme = "README.md"
keywords = ["atm", "agent", "messaging"]     # crate-specific
categories = ["development-tools"]            # or appropriate
```

## Preflight Script

Add or extend `.just/preflight.py` to validate:

1. `cargo publish --dry-run --manifest-path crates/<name>/Cargo.toml` succeeds for every publishable crate
2. Every publishable crate has non-empty `description`
3. Every publishable crate has `license` field
4. Every publishable crate has `repository` field
5. Non-publishable crates (`publish = false`) are explicitly noted and skipped

The preflight gate runs before any `cargo publish` or release workflow.

## Non-Publishable Crates

These crates carry `publish = false` and are skipped by preflight:

- internal test crates
- dev-only crates not intended for crates.io

## Deliverables

- `python3 .just/preflight.py` exits 0 for all publishable crates
- every publishable crate has required metadata
- dry-run publish succeeds for all publishable crates
- release documentation reflects preflight gate

## Required Validation

- `python3 .just/preflight.py` exits 0
- `cargo publish --dry-run` succeeds per-crate for every publishable crate
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`

# Pre-Publish Shared Dependency Strategy

This document defines the temporary dependency strategy used while
`sc-observability` is not yet published on crates.io.

## 1. Scope

This strategy applies only to the pre-publish period for:

- `sc-observability`
- `sc-observability-types`

Committed ATM source must target the real crate names and must not hardcode a
developer-specific filesystem path.

## 2. Publication Status

`sc-observability` `1.0.0` and `sc-observability-types` `1.0.0` are now
published on crates.io.

The temporary pre-publish override strategy is complete:

- the committed root `[patch.crates-io]` override has been removed
- workspace and crate manifests should use the published crates.io packages
  directly
- no branch should reintroduce the old git-source override unless a new,
  explicitly documented pre-publish exception is approved

## 3. Current Dependency Rule

ATM must depend on the published crates.io packages directly:

- `sc-observability = "1.0.0"` (or a later approved released version)
- `sc-observability-types = "1.0.0"` (or a later approved released version)

`Cargo.lock` should therefore resolve both shared crates from crates.io rather
than git or a local path.

## 4. Toolchain Alignment

All local and CI builds participating in this pre-publish strategy must use the
same pinned Rust toolchain:

- channel `1.94.1`
- profile `minimal`
- components `clippy`, `rustfmt`

This matches the repo `rust-toolchain.toml` and CI configuration for Phase K
Sprint K.1.

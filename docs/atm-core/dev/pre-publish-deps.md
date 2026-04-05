# Pre-Publish Shared Dependency Strategy

This document defines the temporary dependency strategy used while
`sc-observability` is not yet published on crates.io.

## 1. Scope

This strategy applies only to the pre-publish period for:

- `sc-observability`
- `sc-observability-types`

Committed ATM source must target the real crate names and must not hardcode a
developer-specific filesystem path.

## 2. Local Developer Strategy

Local developer builds may point the shared crates at a sibling checkout using
an uncommitted `[patch.crates-io]` section in the root `Cargo.toml`.

Example local-only patch:

```toml
[patch.crates-io]
sc-observability = { path = "../sc-observability/crates/sc-observability" }
sc-observability-types = { path = "../sc-observability/crates/sc-observability-types" }
```

Required rules:

- the patch must point to a sibling checkout such as `../sc-observability`
- the patch must remain a local developer edit and must not be committed
- no committed docs, manifests, or scripts may require an absolute path such as
  `/Users/...`

Developers who want a stashable helper file may keep the same snippet in an
ignored file such as `Cargo.local.toml` and apply it locally as needed. That
helper file is ignored by this repo and must not be referenced by committed CI
steps.

## 3. CI Strategy Before Publication

CI must exercise the same pre-publish dependency shape without relying on a
developer workstation path.

The expected CI strategy is:

1. check out `atm-core`
2. check out `sc-observability` as a sibling repository
3. generate or apply a CI-only overlay containing the same `[patch.crates-io]`
   stanza with sibling-relative paths
4. run cargo with the pinned repo toolchain

An example CI-only overlay file is `Cargo.toml.ci`, which is intentionally
ignored by this repo:

```toml
[patch.crates-io]
sc-observability = { path = "../sc-observability/crates/sc-observability" }
sc-observability-types = { path = "../sc-observability/crates/sc-observability-types" }
```

Required rules:

- the CI overlay may use sibling-relative paths only
- the CI overlay must be generated or applied by CI and must not be committed
- once `sc-observability` is published, this overlay strategy should be
  removed in favor of versioned crate dependencies

## 4. Toolchain Alignment

All local and CI builds participating in this pre-publish strategy must use the
same pinned Rust toolchain:

- channel `1.94.1`
- profile `minimal`
- components `clippy`, `rustfmt`

This matches the repo `rust-toolchain.toml` and CI configuration for Phase K
Sprint K.1.

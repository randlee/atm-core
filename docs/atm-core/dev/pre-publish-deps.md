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

Integration branches (`integrate/phase-K` and later) use a committed
git-source `[patch.crates-io]` in the root `Cargo.toml` so that CI runners
can resolve `sc-observability` without a local sibling checkout.

Example (what is committed on `integrate/phase-K`):

```toml
[patch.crates-io]
sc-observability = { git = "https://github.com/randlee/sc-observability", rev = "<sha>" }
sc-observability-types = { git = "https://github.com/randlee/sc-observability", rev = "<sha>" }
```

Required rules:

- use a pinned `rev` so builds are reproducible; update the rev when picking
  up new upstream sc-observability commits (e.g. between Phase L sprints)
- the git URL must be the public GitHub remote — no local paths
- this committed patch is only appropriate on integration/feature branches;
  it must not appear on `develop` or `main`
- remove this section entirely in Sprint L.4 when switching to the published
  crates.io `^1.0.0` release

Local developer builds may still override the git source with a local sibling
path for faster iteration (see §2 above); the local override takes precedence
over the committed git source when both are present.

## 4. Toolchain Alignment

All local and CI builds participating in this pre-publish strategy must use the
same pinned Rust toolchain:

- channel `1.94.1`
- profile `minimal`
- components `clippy`, `rustfmt`

This matches the repo `rust-toolchain.toml` and CI configuration for Phase K
Sprint K.1.

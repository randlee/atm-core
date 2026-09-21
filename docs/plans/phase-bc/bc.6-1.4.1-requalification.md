---
status: complete
branch: feature/bc6-sc-observability-1-4-1
worktree: /Users/randlee/github/atm-core-worktrees/feature/bc6-sc-observability-1-4-1
---

# BC.6 sc-observability 1.4.1 requalification

## Scope and provenance

This append-only layer repins the frozen BC.1 consumer from the coordinated,
code-compatible 1.4.0 family to 1.4.1. No ATM production Rust source changed.
The authoritative upstream source is `c578912653233c7dc678fefe5af575118dbbaaa1`;
the npm recovery is `8bd3d3c56b73f5ccddc58b27dd1eba5dbebd0dfe`.

Upstream reports all ten Rust crates and PyPI `sc-observability` 1.4.1
published and verified. The recovered npm package is
`@synaptic-canvas/sc-observability@1.4.1`, latest `1.4.1`, with tarball SHA256
`d93d38568f8f86b56266fafc12e802de579666ba5ed6c1922492bb9dce0916c9`.
Publication workflow: <https://github.com/randlee/sc-observability/actions/runs/35549256399>.
Corrected npm evidence: <https://github.com/randlee/sc-observability/pull/198#issuecomment-5754069077>.

## Consumed lockfile identities

| package | version | source | Cargo.lock checksum |
| --- | --- | --- | --- |
| `sc-observability` | 1.4.1 | crates.io registry | `4bc4d2a70f2c76cae4c02ed4c497baf658e1db5c13edb40f1d00b9a624f0056a` |
| `sc-observability-types` | 1.4.1 | crates.io registry | `19c1b65c4415266aa9f70319cc70f06686b3b173377f8a12edec5c9f283d087b` |

Both direct dependencies are exact `=1.4.1`; `cargo tree -i` shows one
resolved generation of each. No `sc-observability-tokio` dependency,
`sc-observability-log` global initialization, or proc-macro implementation
dependency was added.

## Source/API compatibility review

The upstream 1.4.0-to-1.4.1 diff contains release metadata, coordinated
manifest/lockfile/version references, changelog/release notes, and publication
policy updates only. There are no Rust implementation source changes in the
published upstream commit. Therefore the BC.1 compatibility fixture and ATM
public behavior require no adaptation.

## Validation and gates

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo check --workspace --locked`: passed.
- `cargo test -p atm-observability --locked`: passed, including the published
  consumer surface, fault injection, health, query, flush, and shutdown tests.
- `cargo test --workspace --locked`: passed.
- `cargo tree -i sc-observability@1.4.1` and `cargo tree -i sc-observability-types@1.4.1`: passed, one generation each.
- `cargo metadata --locked --no-deps`: passed.
- `git diff --check`: passed.

The exact consumed lockfile closes u5 against the published Rust family and
the independently evidenced npm recovery. The change set is limited to
dependency metadata, lockfile, current references, and this evidence artifact.

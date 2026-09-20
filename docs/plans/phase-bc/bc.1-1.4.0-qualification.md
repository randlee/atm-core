# BC.1 sc-observability 1.4.0 qualification

status: complete
branch: feature/bc1-sc-observability-1-4-0
worktree: /Users/randlee/github/atm-core-worktrees/feature/bc1-sc-observability-1-4-0

## Scope

BC.1 is a compatibility-only dependency qualification. ATM retains its
`ObservabilityPort`, retained JSONL policy, tracing bridge, doctor projection,
and CLI durability behavior. No `sc-observability-log` global initialization,
macro migration, Tokio logging crate, typed cleanup, or bridge replacement is
introduced.

## Exact published dependencies

`Cargo.lock` resolves one generation of each direct crate:

| crate | version | source | checksum |
|---|---:|---|---|
| `sc-observability` | 1.4.0 | registry+https://github.com/rust-lang/crates.io-index | `0df8a40ea976a52ea46d21f1dbf97395ff5b18c4183e8565d412f1111e53e46e` |
| `sc-observability-types` | 1.4.0 | registry+https://github.com/rust-lang/crates.io-index | `8ebdaf90fe1ae45cf04ff5e45ac7f78a2aa7374a8090c420a6da61ac039d0612` |

The workspace pins both dependencies exactly (`=1.4.0`). `cargo tree`/metadata
validation showed no duplicate `sc-observability-types` generation.

## Consumer fixture

`crates/atm-observability/src/lib.rs` now contains
`published_1_4_0_consumer_surface_preserves_logger_contract`, which compiles
and exercises the current facade against the published crates: logger
configuration and retained policy, health projection, typed command admission,
flush, shutdown, queue-full fault injection, and the shared query surface.
It deliberately uses the existing ATM adapter and does not initialize the new
global log bridge.

## Validation

- `cargo check --workspace --locked` — passed.
- `cargo test -p atm-observability published_1_4_0_consumer_surface_preserves_logger_contract --locked` — passed (1 test).
- `cargo metadata --locked --no-deps --format-version 1` — passed.
- `cargo tree`/lockfile inspection — exact 1.4.0 direct resolution; no duplicate types crate.
- `git diff --check` — passed.

The full phase-stack validation lanes remain CI/consumer gates; this artifact
records the dependency and focused consumer qualification performed by BC.1.

## Historical preservation

Existing 1.2.0 phase-AA/AQ evidence and release records are not rewritten.
Only current normative requirements, daemon logging documentation, and the
phase-BC current fact were advanced to 1.4.0.

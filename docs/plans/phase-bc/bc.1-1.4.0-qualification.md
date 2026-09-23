# bc.1 sc-observability 1.4.0 qualification

status: complete
branch: fix/bc1-review-2
worktree: /Users/randlee/github/atm-core-worktrees/fix/bc1-review-2

## Scope

bc.1 is a compatibility-only dependency qualification. ATM retains its
`ObservabilityPort`, retained JSONL policy, tracing bridge, doctor projection,
and CLI durability behavior. No `sc-observability-log` global initialization,
macro migration, Tokio logging crate, typed cleanup, or bridge replacement is
introduced. The bc.2 typed migration remains the owner of that future work.

## Exact published dependencies

`Cargo.lock` resolves one generation of each direct crate:

| crate | version | source | Cargo.lock checksum | cached archive SHA256 |
|---|---:|---|---|---|
| `sc-observability` | 1.4.0 | registry+https://github.com/rust-lang/crates.io-index | `0df8a40ea976a52ea46d21f1dbf97395ff5b18c4183e8565d412f1111e53e46e` | `0df8a40ea976a52ea46d21f1dbf97395ff5b18c4183e8565d412f1111e53e46e` |
| `sc-observability-types` | 1.4.0 | registry+https://github.com/rust-lang/crates.io-index | `8ebdaf90fe1ae45cf04ff5e45ac7f78a2aa7374a8090c420a6da61ac039d0612` | `8ebdaf90fe1ae45cf04ff5e45ac7f78a2aa7374a8090c420a6da61ac039d0612` |

The archive hashes were produced with `shasum -a 256` from the cached
`*.crate` files under
`~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/`; each exactly
matches its lockfile checksum. The workspace pins both dependencies exactly
(`=1.4.0`). `cargo tree -i` inspection shows one `sc-observability` and one
`sc-observability-types` generation in the workspace graph.

## Consumer fixture

`crates/atm-observability/src/lib.rs` contains
`published_1_4_0_consumer_surface_preserves_logger_contract`, which compiles
and exercises the current facade against the published crates: logger
configuration and retained policy, health projection, typed command admission,
flush, shutdown, queue-full fault injection, and the shared query surface. It
deliberately uses the existing ATM adapter and does not initialize the new
global log bridge.

## Independent offline extracted-package qualification

The cached `sc-observability-1.4.0.crate` and
`sc-observability-types-1.4.0.crate` archives are extracted into an isolated
temporary consumer. From any shell with both archives in the stated Cargo
cache location, copy/paste this complete sequence; it creates the fresh root,
all required directories, both files, and the vendored package tree without
network access:

```sh
set -eu
consumer_root="$(mktemp -d)"
cd "$consumer_root"
mkdir -p src vendor

cat > Cargo.toml <<'EOF'
[package]
name = "bc1-offline-consumer"
version = "0.1.0"
edition = "2021"

[dependencies]
sc-observability = { path = "vendor/sc-observability-1.4.0" }
sc-observability-types = { path = "vendor/sc-observability-types-1.4.0" }

[patch.crates-io]
sc-observability-types = { path = "vendor/sc-observability-types-1.4.0" }
EOF

cat > src/main.rs <<'EOF'
use std::path::PathBuf;

use sc_observability::{Logger, LoggerConfig};
use sc_observability_types::ServiceName;

fn main() {
    let service = ServiceName::new("bc1-offline-consumer").expect("valid service name");
    let config = LoggerConfig::default_for(service, PathBuf::from("target/logs"));
    let builder = Logger::builder_typed(config).expect("published 1.4.0 builder");
    let logger = builder.build_typed().expect("published 1.4.0 logger");
    let _stopped = logger.shutdown();
}
EOF

tar -xzf ~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/sc-observability-1.4.0.crate -C vendor
tar -xzf ~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/sc-observability-types-1.4.0.crate -C vendor
cargo generate-lockfile --offline
cargo check --offline --locked
cargo run --offline --locked --quiet
```

Result: the literal sequence above was executed from a fresh `mktemp -d` root
on this layer; `cargo generate-lockfile --offline`,
`cargo check --offline --locked`, and `cargo run --offline --locked --quiet`
all passed, and the extracted-package consumer compiled and ran successfully.

## Compatibility edit inventory

All compatibility allowances are narrow and explicitly tied to the future bc.2
typed migration; no typed cleanup was performed here:

| location | retained compatibility | scope/reason |
|---|---|---|
| `crates/atm-observability/src/lib.rs` | legacy `flush`, `try_log`, and logger builder calls | individual methods/functions only; preserve ATM's existing facade until bc.2 migrates its error/admission APIs |
| `crates/atm-observability/src/lib.rs` | published consumer qualification builder call | one test function only; the fixture verifies the 1.4.0 compatibility surface and bc.2 owns typed migration |
| `crates/atm/src/main.rs` | CLI logger builder, sink adapter signatures, `ObservabilityPort::emit`, and flush error mapping | individual functions/trait methods only; preserve the synchronous CLI boundary until bc.2 migrates it |

No module- or crate-wide deprecated allowance remains in these edited paths.

## Validation

- `cargo fmt --all --check` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace --locked` — passed.
- `cargo test -p agent-team-mail --test cli_surface --features cli-surface-dump --locked` — passed (2 tests).
- `python3 .just/tests/test_ecosystem_pins.py` — passed (14 tests).
- `python3 .just/check_line_counts.py` — passed (RULE-003).
- `python3 .just/lint_boundaries.py` — passed.
- `python3 scripts/check-function-length.py --base-ref origin/develop` — passed.
- `python3 scripts/check-nudge-taxonomy.py` — passed.
- `python3 .just/check_read_concurrency_gates.py` — passed (pre-cutover activation is expected).
- `python3 .just/run_lint.py identities` — passed.
- `python3 .just/run_lint.py adr-index` — passed.
- `python3 .just/run_lint.py manifests` — passed.
- `cargo metadata --locked --no-deps --format-version 1` — passed.
- `cargo tree -i sc-observability@1.4.0` and `cargo tree -i sc-observability-types@1.4.0` — passed; one generation each.
- `git diff --check` — passed.

The three `just lint ...` wrappers were unavailable in this worktree because
`.bootstrap-venv/bin/python` is not present; their underlying `run_lint.py`
targets passed with the system Python. This is recorded rather than claiming
the missing wrapper environment ran.

## Exact-head cross-platform CI

PR1548 is the exact-head CI source for the lower review layer:

| item | value |
|---|---|
| PR | `1548` |
| head SHA | `3f60f1bffc03019b3870d3e2086f95b440190088` |
| CI run | `35548802599` |

The required actual platform test jobs completed successfully at this exact
head; packaging jobs are not substituted for these results:

| platform test job | job ID | conclusion |
|---|---:|---|
| `Test (ubuntu-latest)` | `106180210764` | `success` |
| `Test (macos-latest)` | `106180210753` | `success` |
| `Test (windows-latest)` | `106180210748` | `success` |

These results establish bc.1's cross-platform acceptance for the published
1.4.0 consumer qualification.

## Historical preservation

Existing 1.2.0 phase-AA/AQ evidence and release records are not rewritten.
The live release-preflight known-good command and live ecosystem fixtures now
use 1.4.0. Older versions remain only in isolated rollback/ambiguity fixtures
that test the validator's downgrade behavior.

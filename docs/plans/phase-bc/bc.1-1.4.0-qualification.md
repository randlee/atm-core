# bc.1 sc-observability 1.4.0 qualification

status: in-review
branch: fix/bc1-review-1
worktree: /Users/randlee/github/atm-core-worktrees/fix/bc1-review-1

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
`sc-observability-types-1.4.0.crate` archives were extracted into an isolated
temporary consumer, with the types archive supplied through `[patch.crates-io]`.
The consumer uses `Logger::builder_typed`, `build_typed`, and `shutdown`.
These commands passed without network access:

```text
tar -xzf ~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/sc-observability-1.4.0.crate -C vendor
tar -xzf ~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/sc-observability-types-1.4.0.crate -C vendor
cargo generate-lockfile --offline
cargo check --offline --locked
cargo run --offline --locked --quiet
```

Result: the extracted-package consumer compiled and ran successfully.

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

Linux and Windows results are not claimed from this macOS worktree; no CI run
is attributed to this artifact.

## Historical preservation

Existing 1.2.0 phase-AA/AQ evidence and release records are not rewritten.
The live release-preflight known-good command and live ecosystem fixtures now
use 1.4.0. Older versions remain only in isolated rollback/ambiguity fixtures
that test the validator's downgrade behavior.

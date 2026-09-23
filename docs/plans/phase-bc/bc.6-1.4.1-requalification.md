---
status: complete
branch: feature/bc6-sc-observability-1-4-1
worktree: /Users/randlee/github/atm-core-worktrees/feature/bc6-sc-observability-1-4-1
---

# bc.6 sc-observability 1.4.1 requalification

## Scope and provenance

This append-only layer repins the frozen bc.1 consumer from the coordinated,
code-compatible 1.4.0 family to 1.4.1. No ATM production Rust source changed.
The authoritative upstream source is `c578912653233c7dc678fefe5af575118dbbaaa1`;
the npm recovery is `8bd3d3c56b73f5ccddc58b27dd1eba5dbebd0dfe`.

Upstream reports all ten Rust crates and PyPI `sc-observability` 1.4.1
published and verified. The recovered npm package is
`@synaptic-canvas/sc-observability@1.4.1`, latest `1.4.1`, with tarball SHA256
`d93d38568f8f86b56266fafc12e802de579666ba5ed6c1922492bb9dce0916c9`.
Publication workflow: <https://github.com/randlee/sc-observability/actions/runs/35549256399>.
Corrected npm evidence: <https://github.com/randlee/sc-observability/pull/198#issuecomment-5754069077>.

## Coordinated publication inventory

This is the coordinated publication inventory; it is intentionally distinct
from the two-package consumed-lockfile table below.

| published package | version | channel | receipt |
| --- | --- | --- | --- |
| `sc-observability-types` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observe` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-otlp` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-log-macros` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-log` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-dto` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-binding-runtime` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-tauri` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability-py` | 1.4.1 | crates.io | immutable `v1.4.1`; root release run `35545988940` |
| `sc-observability==1.4.1` | 1.4.1 | PyPI | production run `35546290779`; `https://pypi.org/pypi/sc-observability/1.4.1/json` returned HTTP 200 with matching name/version |
| `@synaptic-canvas/sc-observability` | 1.4.1 | npm | recovery run `35549256399`, job `106180836515`, recovery commit `8bd3d3c`; SHA256 `d93d38568f8f86b56266fafc12e802de579666ba5ed6c1922492bb9dce0916c9`; registry endpoint returned HTTP 200 with matching name/version |

The upstream preflight run was `35545706394`, and the root release workflow
was `35545988940`, both for the published upstream commit
`c578912653233c7dc678fefe5af575118dbbaaa1`. The immutable GitHub release
`v1.4.1` includes `checksums.txt` with digest
`sha256:eaddac8a49ac6323284e9bf5ca02008536364bd8d272751cf9fbe4c44c86f2c4`.

The ten-crate inventory was independently checked with:

```text
for crate in sc-observability-types sc-observability sc-observe sc-observability-otlp sc-observability-log-macros sc-observability-log sc-observability-dto sc-observability-binding-runtime sc-observability-tauri sc-observability-py; do cargo search "$crate" --limit 1 | grep -F "$crate = \"1.4.1\""; done
```

All ten commands returned `= "1.4.1"`.

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

The reproducible upstream comparison command was:

```text
gh api repos/randlee/sc-observability/compare/v1.4.0...v1.4.1 --jq '{status,ahead_by,behind_by,total_commits,files:[.files[]|{filename,status,additions,deletions,changes}]}'
```

Its receipt was `status=ahead`, `ahead_by=15`, `behind_by=0`,
`total_commits=15`, and no returned changed filename ended in `.rs`. The
changed set contains coordinated metadata, manifest/lockfile/version
references, release notes, publication workflows, and tests/scripts, but no
published Rust implementation-source change. The upstream API-baseline
receipt reports the exact reviewed head, all nine unchanged baseline entries,
and semver validation PASS for all nine:
<https://github.com/randlee/sc-observability/pull/197#issuecomment-5753346862>.
The source/lock review receipt reports the exact six-package staging and
log-import-integrity commands PASS, provenance validation, 93 relevant tests,
and the locked consumer check:
<https://github.com/randlee/sc-observability/pull/197#issuecomment-5752721535>.

Therefore the bc.1 compatibility fixture and ATM public behavior require no
adaptation. The ATM tree has no local proc-macro fixture target; the upstream
PR197 public-API review receipt is the applicable macro/API evidence, and no
unrun local macro command is claimed here.

## Validation and gates

### Executed in this ATM worktree

- `python3 .just/tests/test_ecosystem_pins.py`: passed, 14/14. The live
  1.4.1 mock baseline is current; the only retained 1.4.0 value is the
  isolated duplicate/ambiguity fixture.
- `cargo fmt --all --check`: passed.
- `cargo check --workspace --locked`: passed.
- `cargo test -p atm-observability --locked`: passed, 16 unit tests and one
  doctest.
- `cargo test --workspace --locked`: passed.
- `cargo tree -i sc-observability@1.4.1` and
  `cargo tree -i sc-observability-types@1.4.1`: passed, one generation each.
- `cargo metadata --locked --no-deps --format-version 1`: passed.
- `python3 .just/check_line_counts.py`, `python3 .just/lint_boundaries.py`,
  `python3 scripts/check-function-length.py --base-ref origin/develop`,
  `python3 scripts/check-nudge-taxonomy.py`,
  `python3 .just/check_read_concurrency_gates.py`,
  `python3 .just/run_lint.py identities`,
  `python3 .just/run_lint.py adr-index`, and
  `python3 .just/run_lint.py manifests`: all passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

### bc.3 corrective top-layer receipt

This corrective review layer was evaluated against frozen lower PR1559 final
head `5d560f534c1ce96c381ba3ee8bd8c58d866abe26`; its content receipt remains
`e4089bc2c457243674b0e0056bc789159f89621c`. It does not modify or rebase the
lower bc.2/bc.6 branches.

The exact locked commands and results were:

```text
cargo test --locked -p atm-observability --all-features
# 17 unit tests, 2 macro qualification tests, and 1 doctest passed
cargo test --locked -p agent-team-mail --all-features --bin atm adapter_tests::retained_sink_fault_matrix_exercises_health_query_flush_and_shutdown -- --exact --nocapture
# 1 retained-sink fault-matrix test passed; 313 other ATM unit tests filtered
python3 .just/lint_boundaries.py
# boundaries passed
cargo fmt --all --check
# passed
cargo clippy --workspace --all-targets -- -D warnings
# passed
```

The macro fixture now has positive evidence for a denylisted value being
redacted and explicitly records the upstream retained outcomes for a 40 KiB
value and 16 excess fields. It bounds only the normal events and documents
that ATM rejection/allowlist parity remains unsupported. The focused ATM
matrix executes both injected `degraded` and `unavailable` modes and asserts
health state/detail, emit, explicit flush, isolated query, and terminal
shutdown. Queue-full classification remains covered by the locked
`atm-observability` typed-facade and tracing-bridge tests; it is not claimed
as an outcome of the sink-health override itself.

### Exact lower-branch CI receipt

PR1552 run `35551718741` completed successfully against the frozen head
`1a7c50a1a59afc3ff28edbc8acf5b4391440dd14`. The format,
lint/classification, source-distribution, macOS/musl/manylinux/Windows wheel,
Clippy, ABI3, and all three Test jobs passed:

- Test Ubuntu job `106188365576`: success.
- Test macOS job `106188365635`: success.
- Test Windows job `106188365629`: success.

This receipt is for the lower implementation branch only; this review layer
does not modify that branch. No ATM production Rust change is included here:
the review-layer diff is limited to the current preflight reference, its
focused test fixtures, the project-plan gate state, and this evidence
artifact.

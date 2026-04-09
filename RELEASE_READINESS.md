# Release Readiness

Branch: `feature/pN-s5-release-readiness`

Scope covered by this record:
- `agent-team-mail-core`
- `agent-team-mail`
- GitHub Release archives for `atm`
- Homebrew formula updates
- `winget` submission readiness

## Summary

Status: `PARTIAL / BLOCKED BY CRATES.IO STAGING ORDER`

The retained release surface and automation are in place, and the workspace
passes the normal quality gates. The remaining blocker is the expected
crates.io dependency staging constraint:

- `agent-team-mail-core 1.0.0` is not yet published on crates.io
- `agent-team-mail` depends on `agent-team-mail-core = ^1.0.0`
- therefore `cargo package -p agent-team-mail --locked` and
  `cargo publish --dry-run -p agent-team-mail --locked --no-verify` fail
  against the crates.io index before the core crate is published

This is the same staged-order constraint previously identified during N.1. It
is not a source-code defect in the CLI crate.

## Cargo Validation Results

| Check | Result | Notes |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | workspace clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | no warnings |
| `cargo test --workspace` | PASS | workspace tests passed |
| `cargo package -p agent-team-mail-core --locked` | PASS | packaged and verified successfully |
| `cargo package -p agent-team-mail --locked` | FAIL | crates.io has no `agent-team-mail-core 1.0.0` yet |
| `cargo publish --dry-run -p agent-team-mail-core --locked --no-verify` | PASS | packaged upload dry-run succeeded |
| `cargo publish --dry-run -p agent-team-mail --locked --no-verify` | FAIL | crates.io has no `agent-team-mail-core 1.0.0` yet |

Exact CLI crate failure:

```text
failed to select a version for the requirement `agent-team-mail-core = "^1.0.0"`
candidate versions found which didn't match: 0.45.2, 0.45.1, 0.44.9, ...
location searched: crates.io index
```

## Install Smoke Test

Smoke test command:

```bash
cargo install --path crates/atm --bin atm --root target/install-smoke --locked
target/install-smoke/bin/atm --help
```

Result:
- PASS
- installed binary entrypoint is `atm`
- no test-only binary was installed

## Retained Release Surface Audit

Verified:
- `release/publish-artifacts.toml` lists only:
  - `agent-team-mail-core`
  - `agent-team-mail`
- `.github/workflows/release.yml` covers:
  - crates.io publish ordering
  - GitHub Release archive generation for `atm`
  - Homebrew formula updates
  - `winget` submission via `vedantmgoyal2009/winget-releaser@v2`
- `.claude/agents/publisher.md` covers the same retained release channels and
  does not depend on removed legacy crates
- `docs/WINGET_SETUP.md` records the one-time bootstrap requirement and the
  review model

## winget Readiness Note

`winget` readiness is evaluated by successful submission/manifests, not same-day
public installability.

Microsoft review normally adds a 1-2 day delay between submission and public
`winget install` visibility. That delay is normal and must not be treated as a
release failure.

## Phase N Completion Gate Assessment

What is complete:
- package identity replacement
- release automation port
- publisher agent port
- customer-facing README rewrite
- retained release-surface and channel audit
- install smoke validation

What remains blocked:
- successful CLI package and CLI publish dry-run against crates.io before
  `agent-team-mail-core 1.0.0` exists on the index

## Unblock Condition

Phase N can fully pass once one of the following is true:

1. `agent-team-mail-core 1.0.0` is actually published to crates.io, then the
   CLI package and CLI dry-run can be rerun in staged order.
2. The release plan is explicitly revised to treat the CLI dry-run as a
   post-core-publish gate rather than a pre-publish readiness gate.

---
title: "fix: v1.2.0 publish surface — expand to all 7 shippable crates"
status: complete
branch: fix/internal-publish-flags
worktree: /Users/randlee/Documents/github/atm-core-worktrees/fix/internal-publish-flags
phase: fix
sprint: publish-surface-v1.2.0
---

# v1.2.0 Publish Surface Fix

## Scope

Correct `release/publish-artifacts.toml` to cover all 7 crates that must be
published to crates.io for v1.2.0. Gate 4 internal-only crates from publication
via `publish = false` in their Cargo.toml.

Prior state: manifest listed only 2 crates (agent-team-mail-core, agent-team-mail).
Five publishable crates were missing. Four internal crates lacked `publish = false`.

## Deliverables

1. **`release/publish-artifacts.toml`** — 7 `[[crates]]` entries in dependency order;
   2 `[[release_binaries]]` entries (`atm`, `atm-daemon`)
2. **`crates/sc-lint-attributes/Cargo.toml`** — `publish = false` added (sc-lint repo publishes separately)
3. **`crates/sc-lint-directives/Cargo.toml`** — `publish = false` added (sc-lint repo publishes separately)
4. **`crates/sc-lint-boundary/Cargo.toml`** — `publish = false` added (sc-lint repo publishes separately)
5. **`crates/atm-runtime-test-support/Cargo.toml`** — `publish = false` added (test infra only)

## Acceptance Criteria

- All 7 publishable crates present in manifest with no `publish = false` in their Cargo.toml
- All 4 internal crates have `publish = false` in their `[package]` section
- `scripts/release_artifacts.py validate-manifest` exits 0
- `scripts/release_artifacts.py validate-preflight-checks` exits 0
- `scripts/release_artifacts.py validate-publish-order` exits 0
- Publish order follows workspace dependency graph:
  agent-team-mail-core(1) → atm-rusqlite(2) → atm-daemon-client(3) →
  atm-daemon-bootstrap(4) → atm-daemon(5) → atm-graft(6) → agent-team-mail(7)
- All 7 crates use `preflight_check = "locked"` (all have workspace path deps)
- `[[release_binaries]]` lists both `atm` and `atm-daemon`
- No Rust code changes in this fix (config + Cargo.toml only)

## Workspace version

`[workspace.package] version = "1.2.0"` in root `Cargo.toml`.

## Notes

- This is a config-only fix branch off `main`, not a phase sprint.
  The `docs/project-plan.md` inclusion gate does not apply.
- `atm-runtime-test-support` appears only in `[dev-dependencies]` of atm-rusqlite
  and atm-daemon — confirmed it does NOT block preflight or publish-order checks.
- `atm-daemon` intentionally depends on `atm-rusqlite` (SQLite coupling from commit
  34467437). Decoupling is deferred to a post-v1.2.0 sprint. Publishing as-is is
  authorized.

## User-Doc Freshness Gate

The retained publish surface now includes the installed end-user markdown corpus
under `docs/user-documents/`. Release preflight treats that corpus as a
blocking publish artifact.

Required contract:

- every markdown file under `docs/user-documents/` must carry
  `reviewed_for_release: <release version>`
- the value must match the release version under validation exactly
- linked documents must exist and remain relative within the user-doc tree

Canonical validation entrypoint:

- `python3 scripts/validate_release.py validate --version <release version>`

The release-preflight workflow invokes that exact entrypoint, and the user-doc
freshness gate reuses `scripts/verify_user_docs.py` rather than maintaining a
second partial checker.

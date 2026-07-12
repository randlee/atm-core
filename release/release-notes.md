# Release Notes

## Summary
- version: 1.2.3
- release date: 2026-07-02
- release owner: publisher (ATM release execution)

This recovery release preserves the Phase AA, Phase AC, and release-readiness
content already assembled for `v1.2.2`, but cuts a new immutable tag after two
publish-blocking defects were discovered mid-release. `v1.2.2` remains as the
partial, abandoned attempt; `v1.2.3` is the clean tag+publish line.

## Included Changes

### 1.2.3 — Recovery patch release
- Add the missing `description` metadata to `atm-storage-claude` so crates.io
  accepts the crate during ordered publish.
- Replace bash-4-only `mapfile` usage in `release.yml` with bash-3.2-compatible
  loops so macOS packaging works on GitHub-hosted runners.
- Preserve the immutable `v1.2.2` tag and recover with a new `release/v1.2.3`
  branch/tag instead of mutating the failed release attempt.

### 1.2.2 — Phase AC + release readiness
- Converge the storage layer on the `atm-storage` contract and canonical types,
  unifying the SQLite backend and the RPC envelope/domain types on a single
  shared type surface.
- Close out storage cleanup and deletion handling and prove SQL Server readiness
  for the storage backend contract.
- Land the production-readiness boundary fixes so the retained runtime enforces
  the storage boundary through live factory wiring.
- Consolidate release readiness (PR #425): `release_gate.sh` branch-regex
  enforcement, the canonical release validation suite (`validate_release.py`,
  `verify_release_archive.py`), publisher release-branch discipline, and the
  publishing-improvements plan docs.

### 1.2.1 — Phase AA
- Restate the daemon architecture and introduce subsystem doctor traits for
  health/observability of daemon subsystems.
- Clean up the mailbox path and remove the SQLite legacy-compatibility surface.
- Relock and enforce the crate boundaries hardened during Phase AA.
- Upgrade observability across the daemon subsystems.

### Release-branch reconciliation (this release)
- Merged `origin/main` into the release branch to retain the v1.2.0
  publish-surface hotfixes (publish=false version-pin skips in the lint scripts,
  cargo-deny path/wildcard allowance, dual-binary archive fixes) that had not
  flowed to `develop`.
- Removed the `sc-lint-attributes` dependency (and its `sc_lint` attribute call
  sites) from the published `agent-team-mail-core` crate; `sc-lint` packages are
  not publishable from this repository. The published surface no longer leaks an
  internal lint-only crate.
- Retained develop's full 1.2.2 publishable crate set and canonical storage
  types.

## Operator / User Impact
- No user-facing CLI behavior change is introduced by the reconciliation itself.
- Storage/runtime internals were converged onto the `atm-storage` contract
  (Phase AC); existing on-disk mailbox and message flows are preserved.
- The SQLite legacy-compatibility surface was removed (Phase AA); installations
  relying on the retained Claude/SQLite paths continue to work through the
  supported storage backends.
- Windows installation remains first-class via the `winget` package and the
  Windows release archive — no Rust toolchain required.

## Packaging / Distribution Notes
- crates.io: publishes the full retained dependency chain in dependency order —
  `atm-storage`, `agent-team-mail-core`, `atm-storage-rusqlite`,
  `atm-storage-claude`, `atm-daemon-client`, `atm-runtime`,
  `atm-daemon-bootstrap`, `atm-daemon`, `atm-graft`, `agent-team-mail`. Publish
  is idempotent and ordered so each crate's upstream is live before it ships.
- GitHub Releases: `atm` + `atm-daemon` archives for
  `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and
  `x86_64-pc-windows-msvc`, with checksums.
- Installed user documentation ships inside every install/archive at
  `share/doc/atm/`, with the primary entrypoint at `share/doc/atm/README.md`.
  The installed binary/doc relationship is fixed as `<install-root>/bin/atm`
  plus `<install-root>/share/doc/atm/README.md`.
- Homebrew: tap `randlee/homebrew-tap`, formulas `agent-team-mail.rb` and
  `atm.rb` updated to 1.2.3.
- winget: package `randlee.agent-team-mail` at 1.2.3. Microsoft review normally
  delays public `winget install` visibility by 1–2 days; submission success is
  the immediate release signal.

## Known Issues / Waivers
- None. No verification waivers are required for this release.
- Note (non-blocking): a standalone local `cargo publish -p agent-team-mail-core
  --locked --dry-run` fails to resolve `atm-storage v1.2.3` because that new
  crate is not yet on crates.io outside the ordered release run. This is an
  ordering artifact, not a defect — the release workflow publishes `atm-storage`
  first, and CI preflight runs the full package check only on the leaf crate.

## Follow-Up
- After the GitHub Release is created, attach these notes to the release body.
- After `release/v1.2.3` merges back to `main`, ensure a `main -> develop`
  reconciliation PR exists so the publish-surface hotfixes and version updates
  flow back to `develop`.
- Confirm `winget` public visibility 1–2 days post-submission.

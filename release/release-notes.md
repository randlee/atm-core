# Release Notes

## Summary
- version: 1.3.0
- release date: 2026-07-11
- release owner: publisher (ATM release execution)

This release lands the Phase AD corrective line on top of the retained 1.2.x
publish surface. It tightens caller-identity ownership, restores direct
post-send emission, removes retired daemon-side Claude/reconcile paths, restores
Windows daemon CI depth coverage, and converges the Phase AD messaging protocol.
No user-facing CLI behavior change is introduced.

## Included Changes
- Complete the `AD.13`–`AD.30` corrective line: tighten caller-identity
  ownership, restore direct post-send emission, and delete retired daemon-side
  Claude/reconcile paths.
- Restore Windows daemon CI depth coverage for same-host local IPC shutdown,
  injected accept-failure, and post-terminate rejection, without accepting
  flaky or hang-prone test behavior.
- Converge the Phase AD messaging protocol: mailbox peek surface, owner-only
  mutation reset, self-addressed send rejection, self-ack loop termination, and
  historical poison cleanup.
- Close out the rusqlite storage/core coupling remediation and the
  daemon-bootstrap boundary drift plan.
- Retire the `atm-storage-claude` crate along with the daemon-side
  Claude/reconcile paths; it is removed from the workspace and the crates.io
  publish surface for 1.3.0 (it was last published at 1.2.3).

## Operator / User Impact
- No user-facing CLI behavior change is introduced by this release.
- Caller identity is now owned explicitly by the invoking shell/overrides;
  `.atm.toml` is not a caller-identity fallback. Existing on-disk mailbox and
  message flows are preserved.
- Retired daemon-side Claude/reconcile paths are removed; supported storage
  backends and messaging flows continue to work unchanged.
- Windows installation remains first-class via the `winget` package and the
  Windows release archive — no Rust toolchain required.

## Packaging / Distribution Notes
- crates.io: publishes the retained dependency chain in dependency order —
  `atm-storage`, `agent-team-mail-core`, `atm-storage-rusqlite`,
  `atm-daemon-client`, `atm-runtime`, `atm-daemon-bootstrap`, `atm-daemon`,
  `atm-graft`, `agent-team-mail` (9 crates). Publish is idempotent and ordered
  so each crate's upstream is live before it ships. The previously published
  `atm-storage-claude` crate is retired and is not part of the 1.3.0 chain.
- GitHub Releases: `atm` + `atm-daemon` archives for
  `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and
  `x86_64-pc-windows-msvc`, with checksums.
- Installed user documentation ships inside every install/archive at
  `share/doc/atm/`, with the primary entrypoint at `share/doc/atm/README.md`.
  The installed binary/doc relationship is fixed as `<install-root>/bin/atm`
  plus `<install-root>/share/doc/atm/README.md`.
- Homebrew: tap `randlee/homebrew-tap`, formulas `agent-team-mail.rb` and
  `atm.rb` updated to 1.3.0.
- winget: package `randlee.agent-team-mail` at 1.3.0. Microsoft review normally
  delays public `winget install` visibility by 1–2 days; submission success is
  the immediate release signal.

## Known Issues / Waivers
- None. No verification waivers are required for this release.

## Follow-Up
- After the GitHub Release is created, attach these notes to the release body.
- After `release/v1.3.0` merges back to `main`, ensure a `main -> develop`
  reconciliation PR exists so release-window commits and version updates flow
  back to `develop`.
- Confirm `winget` public visibility 1–2 days post-submission.

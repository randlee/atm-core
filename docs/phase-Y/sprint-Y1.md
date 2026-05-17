---
id: Y.1
title: ATM Help And UX Improvements
status: planned
branch: feature/pY-s1-atm-help-and-ux
worktree: ../atm-core-worktrees/feature/pY-s1-atm-help-and-ux
target: integrate/phase-Y
---

# Sprint Y.1 — ATM Help And UX Improvements

## Goal

- land the first approved small implementation slice on the Phase `Y` line
- improve `atm help` and adjacent UX text before broader daemon smoke work
- remove or rewrite help/output wording that still implies obsolete mailbox
  truth or mutable shared-inbox behavior

## Hard Dependencies

- `Y.0` must land on `develop`
- `docs/plan-phase-Y.md`
- `docs/phase-Y/help.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- `GH #83`

## GH #83 Design Ruling (approved by team-lead)

- add explicit `Help(HelpCommand)` variant; keep `disable_help_subcommand = true`
- `atm --help` remains clap-generated command syntax help (unchanged)
- `atm help` is conceptual/product help — a separate product feature
- `atm help <subcommand>` delegates to clap `--help` output first, then may
  append optional prose; clap output is the single source of truth for flag
  docs — no parallel text maintained
- concept topics (no clap equivalent) are authored in the topic registry:
- tier 1 (must ship): `config`, `errors`
- tier 2 (may ship incomplete in v1.1): `hooks`, `identity`, `skills`
- Y.1 delivery scope: `atm help --list`, `atm help <topic>`,
  `atm help <topic> --json`
- location: `crates/atm/src/commands/help.rs` with typed topic registry
- no asset-install/init entanglement in this delivery
- current CLI JSON status:
  - `--json` output already exists on the 9 retained commands
  - no Phase `Y` output retrofit is needed
  - JSON input remains out of scope for `Phase Y` and `Phase Z`
  - `atm help <topic> --json` is only an extension of the existing output
    pattern to the new help command

## Exact Targets

- `crates/atm/src/main.rs`
- `crates/atm/src/commands/mod.rs`
- `crates/atm/src/commands/help.rs`
- `crates/atm/src/output.rs`
- `docs/phase-Y/help.md`
- `docs/atm/commands/help.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/phase-Y/sprint-Y1.md`
- `docs/project-plan.md`

## Required Work

- implement `atm help` per the GH #83 design ruling above
- keep `Y.1` scoped to `atm help` and adjacent wording cleanup only
- do not broaden `Y.1` into general CLI JSON I/O work
- document that the retained command surface already has JSON output on the
  existing 9 commands
- document that structured JSON input is a later follow-up after `Phase Z`,
  not a `Y.1` or `Phase Z` gate
- make daemon + SQLite ownership expectations explicit in user-facing help
  where it prevents operator confusion
- remove or rewrite stale help/output text that suggests:
  - shared inbox JSON is ATM’s mutable source of truth
  - normal commands update old inbox messages in place
  - mailbox locks are the correctness boundary for the daemon line
- keep this sprint narrow; do not absorb boundary refactors here
- record any follow-up UX/help fixes that should roll into `Y.2`

## Acceptance Criteria

- `atm help`, `atm help --list`, `atm help <topic>`, `atm help <topic> --json` all work
- `atm help <subcommand>` output starts with clap `--help` content
- tier-1 concept topics (`config`, `errors`) have authored content
- tier-2 topics (`hooks`, `identity`, `skills`) either have content or are explicitly listed as deferred to `Y.2`
- command help/output no longer makes stale file-SSOT claims
- requirements, architecture, and command docs explicitly describe `atm help`
  as the new additive CLI feature
- `Y.1` planning/docs do not claim missing JSON output on the retained
  commands or JSON-input work inside `Phase Y`
- any intentionally deferred UX/help items are explicitly listed for `Y.2`

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `git diff --check`

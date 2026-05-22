# Phase Z Claude Roster Sync And Restore Write-Up

## Purpose

Document the accepted `Phase Z` follow-on direction for:

- `config.json` ownership
- runtime roster truth
- Claude Code compatibility projection
- backup / restore automation

This write-up captures the user-approved behavior and the current
implementation gaps that the `Z.5` through `Z.8` sprint line must close.

## Accepted Ownership Model

Canonical roster truth:

- ATM roster state in SQLite is authoritative
- runtime membership validation uses ATM roster only
- the public immutable roster surface is named `ClaudeCodeTeamRoster`

`config.json` ownership:

- watcher-owned ingress for external Claude Code team changes
- ATM-owned projection target for ATM member mutations and ATM restore
- doctor comparison surface
- not general runtime truth

## Accepted Runtime Behaviors

### Doctor

`doctor` may read `config.json` to compare it against canonical ATM roster
truth and warn when Claude Code team membership is missing or drifted.

### Retained Runtime Commands

`list`, `read`, `clear`, and `ack` must validate membership against ATM roster
only. They must not read `config.json` for roster truth.

### Claude Harness Send

Claude harness `send` behavior is:

1. write ATM durable state first
2. attempt Claude compatibility inbox write when the target inbox exists
3. after that path is selected, compare the member against `config.json`
4. if the member is missing from `config.json`, return the warning:
   - `'<member-name>' is not on claude code roster <atm-team>/config.json`

That warning must not veto the inbox write once the inbox target exists.

### Watcher Ingest

When a new Claude Code team is ingested, or when the watcher sees a
`config.json` change that was not caused by a daemon-owned write, the watcher /
reconcile lane imports the resulting member state into canonical ATM roster
truth.

## Accepted Team-Admin Behaviors

### `atm members` / `atm teams`

These views should report ATM roster truth. If Claude file state is missing or
drifted, `doctor` reports that discrepancy rather than forcing members / teams
commands to treat `config.json` as authoritative.

### `atm team member add`

The canonical mutation path is:

1. mutate ATM roster truth
2. project the resulting member set into `config.json`

### `atm teams backup`

Backup keeps raw Claude team files and ATM-owned state for:

- audit
- emergency inspection
- manual fallback if needed

Backup does not make backup `config.json` the restore authority.

### `atm teams restore`

Restore becomes an ATM-owned synchronization path:

1. operator removes the stale Claude team from disk
2. operator recreates the team shell through Claude Code `TeamCreate`
3. `atm teams restore` restores approved ATM-owned durable state
4. `atm teams restore` overwrites `config.json` directly from canonical ATM
   roster truth

Restore must preserve:

- current recreated `team-lead` entry
- current recreated `leadSessionId`
- member metadata such as `tmux_pane_id`

Restore must not treat backup `config.json` as the roster source of truth.

## Member Metadata

The following retained Claude Code compatibility fields belong in canonical ATM
roster-member state when still justified:

- `tmux_pane_id`
- other retained per-member routing / harness metadata reviewed as still
  necessary

These fields are:

- imported from `config.json` through watcher ingress when external changes are
  accepted
- projected back into `config.json` when ATM owns the write
- not durably sourced from `.atm.toml`

## Current Production Touchpoints To Remove Or Narrow

Runtime roster-truth reads that should be removed from normal command flows:

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/service_runtime.rs`

Comparison / admin surfaces that must be narrowed to their justified role:

- `crates/atm-core/src/doctor/mod.rs`
  - comparison only
- `crates/atm-core/src/team_admin.rs`
  - ATM-roster views and ATM-owned mutation / projection only
- `crates/atm-core/src/team_admin/restore.rs`
  - ATM-owned restore / projection rather than backup-config replay

Boundary surfaces that must stop behaving like generic roster lookups:

- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`

## Phase Z Follow-On Sprint Split

- `Z.5` runtime roster truth cutover
- `Z.6` watcher-owned config ingress and Claude send warning semantics
- `Z.7` team-admin roster authority and canonical member metadata
- `Z.8` backup / restore automation and config projection

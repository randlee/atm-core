# Phase Z Claude Roster Sync And Restore Write-Up

## Purpose

Document the accepted `Phase Z` follow-on direction for:

- `config.json` ownership
- runtime roster truth
- Claude Code compatibility projection
- backup / restore automation

This write-up captures the user-approved behavior and the current
implementation gaps that the `Z.5` through `Z.10` sprint line must close.

The exact per-path delete / narrow / keep map lives in:

- `docs/phase-Z/config-json-violation-inventory.md`

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
truth and warn when Claude Code team membership is missing or drifted in
either direction.

### Retained Runtime Commands

`list`, `read`, `clear`, and `ack` must validate membership against ATM roster
only. They must not read `config.json` for roster truth.

### Claude Harness Send

Claude harness `send` behavior is:

1. write ATM durable state first
2. attempt Claude compatibility inbox write when the target inbox exists
3. build the immutable `ClaudeCodeTeamRoster` warning snapshot from canonical
   ATM roster truth after the durable write succeeds; do not read
   `config.json` directly for normal member lookup
4. if the member is missing from the Claude roster projection, return the
   warning:
   - `'<member-name>' is not on claude code roster <atm-team>/config.json`
5. if the team `config.json` document is missing entirely, the existing-inbox
   fallback still raises the retained missing-config warning only after the
   durable write path is complete

That warning must not veto the inbox write once the inbox target exists.

### Watcher Ingest

When a new Claude Code team is ingested, or when the watcher sees a
`config.json` change that was not caused by a daemon-owned write, the watcher /
reconcile lane imports the resulting member state into canonical ATM roster
truth.

`Z.8` closes the temporary startup-only bridge:

- `hydrate_roster_from_team_config_once_at_startup_if_empty(...)`

That helper was the last pre-watcher one-shot roster hydration path. It was
deleted once watcher / reconcile became the only production reader of external
Claude roster changes.

`Z.8` also adds daemon-owned write suppression:

- ATM-owned `config.json` projection writes are recorded in one in-memory
  journal keyed by canonical config path plus content digest
- the watcher / reconcile lane consumes one matching entry and suppresses only
  that one self-authored event
- suppression is intentionally process-local; daemon restart clears it and a
  post-crash event falls back to ordinary idempotent external ingest

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

See `docs/phase-Z/config-json-violation-inventory.md` for the authoritative
path inventory, sprint ownership, and exact delete / rewrite expectations.

## Phase Z Follow-On Sprint Split

- `Z.5` retained runtime command cutover to ATM roster truth
- `Z.6` Claude send semantics and immutable `ClaudeCodeTeamRoster`
- `Z.7` config-ingress boundary narrowing, startup-only allowlist, and static
  gate definition
- `Z.8` watcher-owned config ingest and projection-write suppression
- `Z.9` team-admin roster authority and canonical member metadata
- `Z.10` backup / restore automation and config projection

# ADR-016: Claude Config Ingress And Roster Projection Ownership

## Status

Proposed

## Context

`Phase Z` smoke exposed a mismatch between the intended roster architecture and
the current retained command / team-admin implementation:

- canonical delivery-harness resolution already depends on ATM roster truth in
  SQLite
- several retained command paths still read `.claude/teams/<team>/config.json`
  directly for membership validation
- team backup / restore still treats backup `config.json` as a restore input
  rather than treating ATM roster state as the source of truth
- per-member Claude Code compatibility metadata such as `tmux_pane_id` exists
  in `config.json` and must survive ATM-owned member add, ingest, and restore

The intended architecture already says:

- only the owning config-ingress subsystem may parse team `config.json`
- normal runtime paths must not use `config.json` as roster truth
- watcher / reconcile owns external filesystem change handling

The implementation still needs a single accepted ownership decision for:

- who may read `config.json`
- how runtime consumers learn team membership
- when `config.json` differences become warnings instead of hard failures
- how backup / restore interact with ATM roster truth

## Decision

ATM roster state in SQLite is the only canonical team roster truth.

`config.json` is:

- a watcher-owned ingress document for externally created or externally edited
  Claude Code team state
- an ATM-owned projection target when ATM mutates or restores team membership
- a doctor comparison surface for operator diagnostics
- not a general runtime roster-truth dependency

### Ownership Rules

1. Runtime roster truth
- retained runtime command paths use ATM roster truth only
- `list`, `read`, `clear`, and `ack` must not read `config.json` for
  membership validation
- `send` must not use `config.json` as a pre-write membership gate

2. Public roster surface
- runtime consumers learn team membership from one immutable public projection
  named `ClaudeCodeTeamRoster`
- that view is derived from canonical ATM roster state rather than from direct
  `config.json` reads

3. `config.json` read surfaces
- watcher / reconcile may parse `config.json` for external-ingest purposes
- `doctor` may parse `config.json` only to compare it against canonical ATM
  roster truth and report drift
- no other normal runtime path may parse `config.json` for roster truth

4. Claude harness send semantics
- after SQLite write succeeds, the Claude compatibility inbox write is still
  attempted when the target inbox exists
- after the write path selects the Claude harness target, ATM may compare that
  member against `config.json`
- if the member is absent from `config.json`, ATM returns a warning:
  - `'<member-name>' is not on claude code roster <atm-team>/config.json`
- that warning is diagnostic only; it does not veto the inbox write once the
  inbox target exists

5. External `config.json` changes
- when the watcher sees a `config.json` change that was not caused by an
  ATM-owned write, the change flows through the owned watcher / reconcile lane
- that lane performs the equivalent of `atm team member add` / roster import
  into canonical ATM roster truth

6. Member metadata ownership
- official Claude Code roster fields such as `tmux_pane_id` are canonical ATM
  roster-member fields
- they are imported from `config.json` when watcher ingress runs
- they are projected back into `config.json` when ATM owns the write
- `.atm.toml` must not remain the durable source of per-member tmux pane
  routing metadata

7. Backup / restore
- backup preserves raw Claude team files, inboxes, tasks, and ATM-owned durable
  state for audit and emergency inspection
- restore must not replay backup `config.json` as roster truth
- operator recreates the team shell through Claude Code `TeamCreate`
- `atm teams restore` then projects canonical ATM roster state back into the
  recreated team's `config.json`
- restore preserves the current team-lead entry and current `leadSessionId`
  from the recreated team shell while restoring non-lead membership and
  approved durable state

## Consequences

Positive:

- one canonical roster truth for all runtime membership decisions
- watcher / reconcile becomes the only external config-ingest reader
- doctor remains the explicit drift-report surface
- team restore becomes deterministic and ATM-owned rather than a manual
  file-replay procedure
- per-member Claude metadata survives ATM-owned add / ingest / restore flows
- repository-local lint / `sc-lint`-candidate gates can mechanically reject
  new boundary regressions once the current violation inventory is closed

Costs:

- retained runtime helper APIs that expose generic `load_team_config(...)`
  behavior must be narrowed or removed
- team-admin surfaces must be rewritten around ATM roster truth
- restore implementation is a real behavior change, not a wording-only update
- the current violation inventory must be carried as an explicit per-path
  delete / narrow / keep plan rather than loose prose

## Phase Z Implementation Split

This ADR is implemented by `Phase Z` follow-on sprints:

- `Z.5` retained runtime command cutover to ATM roster truth
- `Z.6` Claude send semantics and immutable `ClaudeCodeTeamRoster`
- `Z.7` config-ingress boundary narrowing and static gate definition
- `Z.8` watcher-owned Claude config ingress
- `Z.9` team-admin roster authority and member metadata
- `Z.10` backup / restore automation and config projection

The authoritative path-by-path delete / narrow / keep map is:

- `docs/phase-Z/config-json-violation-inventory.md`

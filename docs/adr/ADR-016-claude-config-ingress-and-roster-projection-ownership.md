# ADR-016: Claude Config Ingress And Roster Projection Ownership

## Status

Accepted

Phase AD narrowing note:
- `ADR-019` retires the daemon watcher/reconcile lane from the accepted
  runtime
- this ADR still governs the rule that SQLite roster state is canonical and
  `config.json` is not runtime roster truth
- any surviving `config.json` import or projection work must be explicit
  CLI/admin behavior rather than background watcher/reconcile behavior

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
- no retained runtime path may treat filesystem watch/reconcile as required
  product behavior

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
- `doctor` may report canonical ATM roster truth without requiring
  `config.json`; any surviving config-file comparison is diagnostic-only and
  must remain outside normal runtime correctness
- explicit CLI/admin repair or import paths may parse `config.json` when they
  are the documented owner of that action
- no background watch/reconcile lane is required by the accepted runtime
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
- external `config.json` changes do not imply a required background daemon
  watch/import lane
- if ATM imports external `config.json` changes, that import must happen
  through one explicit CLI/admin-owned path into canonical ATM roster truth

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
- `atm teams restore` restores compatibility inbox/task artifacts without
  rewriting canonical roster truth from `config.json`
- restore may preserve the recreated shell's existing `config.json` and
  compatibility metadata when those files already exist, but they are not
  rebuilt from canonical roster state on the normal restore path

## Consequences

Positive:

- one canonical roster truth for all runtime membership decisions
- before `ADR-019`, watcher / reconcile was the planned external config-ingest
  reader; that behavior is now historical only
- doctor remains the explicit roster/diagnostic surface without making
  config-file comparison part of runtime correctness
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

## Mechanical Enforcement

The `SCB-CONFIG-*` rule family is a durable architectural enforcement
mechanism, not a one-off review note.

Mechanism:

- repository-local implementation lives first in `.just/lint_boundaries.py`
  and is exercised by `just lint boundaries`
- the first implementation is deterministic token / regex scanning against the
  checked-in source tree plus a checked-in known-bad fixture; it is not a
  prose-only checklist
- once the rule shape is stable, the same semantics may migrate into
  standalone `sc-lint`, but the repo-local gate remains authoritative until
  that migration is accepted

Allowlist contract:

- approved surviving callers are recorded in a checked-in machine-runnable
  file, `.just/allowlists/scb_config_allowlist.toml`
- each allowlist row must record at minimum:
  - `rule`
  - `path`
  - `symbol`
  - `why`
  - `sunset_sprint`
- the markdown planning inventory remains the human-readable source of intent,
  but the TOML allowlist is the machine-readable gate input
- any new allowlist row requires an explicit sprint/ADR justification and a
  named sunset sprint unless the row is part of the final accepted survivor set

Gate failure semantics:

- a real repo violation of `SCB-CONFIG-001`, `SCB-CONFIG-002`, or
  `SCB-CONFIG-003` must make `just lint boundaries` exit non-zero and print
  `SCB-CONFIG-00X <path>:<line> <summary>`
- the lint also runs a known-bad fixture self-test; if the fixture is not
  rejected, the top-level lint must exit non-zero and report the false-negative
- a clean repo passes only when the allowlisted survivors match, the live tree
  has no unexpected violations, and the known-bad fixture is rejected

## Phase Z Implementation Split

Implementation later ran through Phase Z follow-on work, but the durable
contract remains this ADR itself:

- no retained runtime path may treat watcher/reconcile as the required reader
  of Claude config JSON
- ATM roster state is the only runtime roster truth
- Claude projection types stay derivative, not authoritative
- `SCB-CONFIG-*` static gates and their checked-in allowlist define the
  machine-readable survivor set for any remaining config-json touchpoints

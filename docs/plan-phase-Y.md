# Phase Y Plan

## Goal

Prepare the daemon + SQLite mail-SSOT implementation line for release
validation without carrying forward the compatibility-inbox write hazards
that still exist on the current implementation line.

Phase `Y` therefore focuses on architectural cleanup before broad smoke and
dogfood work:

- minimize ATM-authored metadata on the Claude compatibility inbox surface
- move all ATM-authored inbox/config write behavior behind one hard-owned
  boundary
- centralize delivery/write-routing policy into one explicit coordinator
- replace scattered command-local conditionals with auditable event-family
  state machines
- remove mutable workflow-state projection from compatibility inbox output
- eliminate normal-runtime mailbox-lock dependence where the compatibility
  contract allows it
- make all CLI/client-socket write paths and SQLite query state machines
  explicit and QA-auditable

Executable smoke, canary dogfood, and release sign-off move to `Phase Z`
after the `Phase Y` implementation line closes.

## Baseline

- planning branch: `feature/pY-s0-planning`
- current merged implementation baseline: `develop` at `d59ea5f`
- pre-smoke trivial-fixes branch: `feature/pY-trivial-fixes`
- future integration branch: `integrate/phase-Y`

## Pre-Phase Prerequisite

`Y.0` is already in flight on `feature/pY-trivial-fixes` and must land on
`develop` before `integrate/phase-Y` is created:

- close the small pre-smoke fixes and their QA follow-ups
- preserve the current working compatibility inbox write format while
  documenting the real contract
- leave `atm help` implementation for `Y.1`

## Current Compatibility-Inbox Reality

The current code does **not** satisfy the desired end-state yet.

Observed production ATM-authored inbox write entrypoints:

1. `send` command path
   - `crates/atm-core/src/send/mod.rs::persist_message_and_seed_workflow(...)`
   - persists SQLite/workflow state first
   - reaches compatibility rewrite only through the retained runtime refresh owner

2. `ack` reply path
   - `crates/atm-core/src/ack/mod.rs`
   - emits the reply through the same narrowed helper
   - does not rewrite the original source inbox merely because ack state changed

3. compatibility export/source-projection path
   - `crates/atm-core/src/mailbox/store.rs::write_compat_source_projections(...)`
   - this is the writer shape that most closely matches the intended private
     watcher/import/export ownership model

Observed constraints:

- ATM-authored compatibility inbox writes are still full-file atomic rewrites,
  not append-only writes
- those rewrites still sit under mailbox/workflow lock coordination
- command-layer send/ack paths still invoke compatibility inbox writes
  directly through shared helpers
- watcher/reconcile is **not** yet the sole owner of ATM-authored
  compatibility inbox writes

## Desired End-State

Phase `Y` should converge on two allowed Claude-compatibility inbox write
classes only:

1. normal runtime ATM-authored compatibility export
   - owned by one daemon-private watcher/import/export or closely related
     file-writer subsystem
   - synchronized inside that owned subsystem
   - `Claude Code` harness only
   - never selected by model alone; harness type is the gate
   - append-only if the approved compatibility wire format allows it

2. explicit admin / restore / repair staging path
   - not part of normal send/ack/read/clear runtime flow
   - must remain staged, atomic, and separately documented

Normal runtime behavior must therefore converge on:

- one SQLite-backed delivery/state flow for all harnesses
- one Claude-Code-only compatibility append branch after that delivery/state
  flow
- no JSONL append for non-Claude harnesses
- one central delivery-policy coordinator that dispatches by event family and
  `RosterHarness`
- separate event-family state machines rather than one combined “mail send”
  state machine
- required families:
  - `NewMessageStateMachine`
  - `ThreadUpdateStateMachine`
  - `AckReplyStateMachine`
  - `InboxRepairStateMachine`
  - `RestoreInboxRebuildStateMachine`
- one explicit companion error contract when SQLite delivery fails:
  - original outward delivery still proceeds
  - an additional `atm-system@<team>` error message is emitted
  - the nudge path mirrors that two-message behavior
  - no alternate fallback path may replace that companion error emission
- minimal synchronization in the delivery path:
  - canonical roster data should be read as a short-lived snapshot, not held
    as a long-lived coordination lock
  - SQLite transaction scope is the durable mutation boundary
  - compatibility export/nudge runs after the relevant durable decision point
    and must not become part of message-truth locking
  - cross-domain lock nesting between roster state, SQLite durability, and
    compatibility mailbox/workflow state should be eliminated rather than
    carefully expanded

Normal command paths must not own direct compatibility inbox rewrites once the
Phase `Y` line is complete.

## Planning Deliverables Before Sprint Creation

The following must be completed on the planning branch before numbered Phase
`Y` execution begins:

- shared-inbox field inventory plus the decision framework that `Y.5` will use
  to justify each surviving field
- `docs/phase-Y/inbox-field-inventory.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- `docs/phase-Y/delivery-state-machines.md`
- backlog list for any missing command/request/query diagrams
- line-numbered write/removal ledger for every inbox/config write stack
- explicit enum + transition definitions for every required write-affecting
  event-family state machine
- approved implementation scopes for the first two small pre-smoke sprints

These are planning deliverables, not execution sprints.

## Implementation Sprint Sequence

### Y.1 ATM Help And UX Improvements

Purpose:

- land the first approved small implementation slice for the daemon + SQLite
  release line
- improve `atm help` and adjacent user-facing guidance before broader smoke
  work begins
- remove stale wording that still implies obsolete file-SSOT or mutable
  shared-inbox behavior

### Y.2 Pre-Smoke Easy Fixes And Validation

Purpose:

- land the second approved small implementation slice before heavy smoke work
- close only narrow, low-risk fixes that are explicitly identified during
  planning and `Y.1`
- keep this sprint intentionally small; do not absorb boundary refactors or
  rollout/discovery work here

### Y.3 Hard Write-Boundary Consolidation

Purpose:

- move all ATM-authored compatibility inbox/config writes behind one hard
  owner boundary
- make the owner daemon-private or tightly coupled to the watcher/import/export
  subsystem
- add machine-checkable enforcement so command code cannot bypass that owner
- execute the line-numbered removal ledger from
  `docs/phase-Y/inbox-write-path-audit.md`

Current status:

- complete on `feature/pY-s3-hard-write-boundary-consolidation`
- normal retained `send` now persists SQLite/workflow state first and reaches
  compatibility rewrite only through
  `RetainedServiceRuntime::refresh_compat_inbox_projection(...)`
- `ack` and `clear` no longer own source-inbox compatibility rewrites

### Y.4 Delivery Coordinator And Event-Family State Machines

Purpose:

- land the central delivery-policy coordinator after the write owner boundary is
  reduced to the approved shape
- replace scattered command/daemon delivery branches with explicit
  event-family state machines
- make harness routing, failure behavior, and transition observability auditable
  in one place

Current status:

- complete on `feature/pY-s4-delivery-coordinator-and-state-machines`
- the retained coordinator/state-machine seam now lives in
  `crates/atm-core/src/delivery_policy.rs`
- retained `send` and `ack` now resolve copied roster snapshots through
  `RetainedServiceRuntime::load_roster_member(...)`
- retained compatibility export now remains enabled only for `Claude Code`
  harnesses; non-Claude harnesses skip ATM-authored JSONL export

### Y.5 Mutable Compatibility-Field Removal And Dependency Exposure

Purpose:

- remove mutable ATM workflow-state fields from compatibility output
- let breakage expose hidden logic dependencies
- delete or refactor those obsolete dependencies instead of preserving them
- justify every remaining compatibility field explicitly
- keep field-removal logic inside the event-family state machines rather than
  in generic export conditionals

### Y.6 Append-Only Compatibility Export Cutover

Purpose:

- if the approved compatibility wire contract allows it, replace full-file
  compatibility rewrites with append-only writes
- eliminate normal-runtime inbox locks from the hot write path
- keep restore/repair flows separate if they still require staged rewrites
- keep the append/no-append decision inside the harness-specific new-message
  state machines rather than in scattered writer call sites

## Phase Boundary

`Phase Y` closes when:

- the hard-owned write boundary is enforced
- the delivery-policy coordinator and event-family state machines are landed
- mutable compatibility fields are reduced to the approved set
- append-only compatibility export is either implemented or explicitly rejected
  by the approved wire-contract decision

The progressive executable smoke, `atm-dev` canary, and release sign-off work
then moves to `docs/plan-phase-Z.md`.

## Phase Rules

- do not preserve mutable ATM workflow state in the compatibility inbox merely
  because older code still consumes it
- use data removal to expose hidden dependencies early
- every write-stack removal or retention decision must be documented by file,
  line, and function name before the corresponding sprint starts
- every write-affecting event family must have its own documented state machine
  with:
  - explicit enum values
  - explicit transition table
  - explicit side effects
  - explicit observable transition names
- delivery-state-machine design must prefer snapshots and post-commit
  side effects over new application-level lock hierarchies
- the planner should treat “who sees a membership change first” as low-value
  race handling:
  - if a message lands just before or just after a member is removed, that
    edge is not a reason to add broad coordination locks
  - if a member is added and a message is sent immediately after, correct
    queueing and durable write sequencing should handle it without global
    locking
- harness-specific branching must live in the delivery-policy coordinator plus
  the relevant state machine, not in command code
- do not treat command-path direct inbox rewrites as an acceptable long-term
  boundary outcome
- every CLI command, every client-socket command family, and every SQLite
  query used by the runtime must have a documented state machine or query
  diagram before Phase `Y` closes
- QA must verify both:
  - the implementation matches the documented state machines
  - no undocumented compatibility write path survives

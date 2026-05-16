# Phase Y Plan

## Goal

Prepare the first daemon + SQLite mail-SSOT release for real operator use
 without carrying forward the compatibility-inbox write hazards that still
 exist on the current implementation line.

Phase `Y` therefore starts with architectural cleanup before broad smoke and
dogfood work:

- minimize ATM-authored metadata on the Claude compatibility inbox surface
- move all ATM-authored inbox/config write behavior behind one hard-owned
  boundary
- remove mutable workflow-state projection from compatibility inbox output
- eliminate normal-runtime mailbox-lock dependence where the compatibility
  contract allows it
- make all CLI/client-socket write paths and SQLite query state machines
  explicit and QA-auditable

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
   - `crates/atm-core/src/send/mod.rs::append_mailbox_message_and_seed_workflow(...)`
   - rebuilds the mailbox projection from SQLite metadata/records
   - rewrites the full compatibility inbox file

2. `ack` reply path
   - `crates/atm-core/src/ack/mod.rs`
   - emits the reply through the same helper
   - therefore still rewrites the compatibility inbox file

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
   - append-only if the approved compatibility wire format allows it

2. explicit admin / restore / repair staging path
   - not part of normal send/ack/read/clear runtime flow
   - must remain staged, atomic, and separately documented

Normal command paths must not own direct compatibility inbox rewrites once the
Phase `Y` line is complete.

## Planning Deliverables Before Sprint Creation

The following must be completed on the planning branch before numbered Phase
`Y` execution begins:

- minimum-field decision for ATM-authored shared-inbox metadata
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- backlog list for any missing command/request/query diagrams
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

### Y.4 Mutable Compatibility-Field Removal And Dependency Exposure

Purpose:

- remove mutable ATM workflow-state fields from compatibility output
- let breakage expose hidden logic dependencies
- delete or refactor those obsolete dependencies instead of preserving them

### Y.5 Append-Only Compatibility Export Cutover

Purpose:

- if the approved compatibility wire contract allows it, replace full-file
  compatibility rewrites with append-only writes
- eliminate normal-runtime inbox locks from the hot write path
- keep restore/repair flows separate if they still require staged rewrites

### Y.6 Smoke Bring-Up

Purpose:

- developer-coordinated daemon bring-up
- feature-by-feature executable smoke pass
- corner-case and recovery verification on the real binaries

### Y.7 Fix And Revalidate

Purpose:

- close smoke findings from `Y.6`
- re-run full executable validation on the fixed branch

### Y.8 `atm-dev` Canary / Dogfood

Purpose:

- move from single-operator smoke to `atm-dev` team use on the new binaries
- verify UX, recovery text, and operational behavior under real use

### Y.9 Final Fixes And Release Sign-Off

Purpose:

- close `Y.8` findings
- produce the final release-readiness verdict

## Phase Rules

- do not preserve mutable ATM workflow state in the compatibility inbox merely
  because older code still consumes it
- use data removal to expose hidden dependencies early
- do not treat command-path direct inbox rewrites as an acceptable long-term
  boundary outcome
- every CLI command, every client-socket command family, and every SQLite
  query used by the runtime must have a documented state machine or query
  diagram before Phase `Y` closes
- QA must verify both:
  - the implementation matches the documented state machines
  - no undocumented compatibility write path survives

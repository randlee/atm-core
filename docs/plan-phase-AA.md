# Phase AA Plan

## Goal

Remove every concrete SQLite reference from `atm-daemon` and restore the
daemon to the original simple role:

- singleton host/runtime ownership
- transport routing
- bounded request dispatch
- minor runtime error handling

`Phase AA` exists because the current daemon line drifted across the storage
boundary and accumulated SQLite-aware composition, health, observability, and
test support that do not belong in a thin router.

The required end state is:

- concrete SQLite construction moves to a dedicated `atm-runtime` composition
  crate
- existing storage-facing capability traits live in `atm-core` and remain
  backend-neutral:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
- Phase AA adds subsystem doctor traits beside those boundaries instead of
  introducing a second parallel storage trait family
- `atm-daemon` knows only storage-neutral `atm-core` ports
- `atm doctor` regains a direct local diagnostics path that can inspect
  SQLite/store health without requiring daemon routing
- daemon-routed doctor data remains optional and additive, used only for
  daemon-only runtime state or for faster asynchronous answers when the daemon
  is already live

## Baseline

Planning branch:
- `plan/remove-sqlite-from-daemon`

Planning worktree:
- `../atm-core-worktrees/plan/remove-sqlite-from-daemon`

Expected execution integration branch:
- `integrate/phase-AA`

Current violated state that `Phase AA` must delete:
- `atm-daemon` directly depends on `atm-rusqlite`
- daemon composition constructs `SqliteBoundaryAssembly`
- daemon runtime-health code reads SQLite-backed roster state directly
- daemon owns SQLite-specific observability glue
- daemon test support composes SQLite assemblies directly

Authoritative planning support artifacts:
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `.claude/agents/boundary-guard.md`

## Why Phase AA Exists

The daemon was intended to be simple and storage-agnostic. The boundary was
created specifically so the daemon would never know that the first storage
adapter is SQLite.

That invariant no longer holds. Once the boundary widened, daemon code started
to accumulate concrete adapter knowledge instead of staying a thin router.
`Phase AA` is not feature expansion; it is architectural damage removal.

## Target Architecture

### `atm-runtime` becomes the concrete composition root

`Phase AA` introduces a dedicated `atm-runtime` crate.

`atm-runtime` owns:
- construction of `SqliteBoundaryAssembly`
- injection of SQLite-specific observability into `atm-rusqlite`
- assembly of the concrete production `MailStore`, `RosterStore`, and replay
  store implementations
- any direct local doctor helper that must inspect the installed storage
  adapter
- installation of the active capability-trait implementations used by the CLI
  and daemon

`atm-runtime` does not own:
- CLI parsing/rendering
- daemon transport
- workflow/state-machine business logic

### `atm-daemon` becomes a thin router again

After `Phase AA`, `atm-daemon` owns only:
- singleton startup / shutdown
- local and remote transport hosting
- request validation and routing
- bounded dispatch / reply handling
- daemon-only runtime state that truly cannot be answered outside the daemon

`atm-daemon` must not own:
- SQLite construction
- SQLite observability adapters
- SQLite health probing
- SQLite replay-store implementations
- SQLite-specific test helpers
- backend identity branching such as "if SQLite then ..."

### Storage capability traits replace backend-shaped seams

`Phase AA` should not replace SQLite-specific code with one giant generic
`Storage` trait. The Phase AA implementation decision is:

- keep the existing storage-neutral `atm-core` boundaries as the primary
  storage capability surfaces:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
- add subsystem doctor traits beside those boundaries rather than introducing a
  second parallel read/write trait hierarchy in the same phase
- if a narrower reader/writer split is still desirable after the daemon is
  clean, it becomes a later simplification line rather than extra churn inside
  Phase AA

Rules:
- traits are named for the behavior they expose, not for backend identity
- traits stay small and composable
- daemon and CLI depend only on the capabilities they actually need
- backend implementations may satisfy multiple traits, but callers should not
  receive a giant god-interface just because one backend can do many things
- both SQLite-backed and Claude-JSON-backed implementations are allowed to
  satisfy the same capability family
- `Phase AA` reuses `MailStore` / `TaskStore` / `RosterStore` as the primary
  storage-neutral capability surfaces and does not create parallel
  `MessageReader` / `MessageWriter` / `RosterReader` / `RosterWriter` traits
  in the same phase

### `atm doctor` becomes direct-first, daemon-optional

The baseline `atm doctor` path should be local and direct:
- config discovery
- team/identity resolution
- direct store-path / SQLite health
- roster/store consistency checks that do not require a running daemon

The daemon path is additive:
- singleton owner state
- live in-memory daemon status cache
- active background worker state
- degraded runtime conditions that exist only while the daemon is live
- cross-subsystem drift findings such as SQLite roster vs `config.json` roster
  mismatch

The routing rule is therefore:
- if a check can be answered directly from local config/store state, the CLI
  may query it directly
- the daemon is used only when daemon-owned runtime state is required or when
  an already-live daemon can answer faster/asynchronously

The diagnostic ownership rule is:
- deep investigation belongs to the implementing subsystem
- aggregation and cross-subsystem comparison belong to the caller
- the daemon may compare subsystem reports, but it must not reimplement
  backend-specific diagnosis logic
- config diagnostics follow the same rule: backend-specific `config.json`
  investigation belongs behind a config doctor trait rather than ad hoc logic
  buried in daemon code
- daemon runtime snapshots remain daemon-owned; backend-specific store health
  leaves the runtime snapshot and appears only through subsystem doctor reports

## Simplicity Contract

The daemon must remain auditable through a very small state-machine set.

`Phase AA` closure requires an explicit daemon state-machine inventory with no
more than five top-level machines:

1. bootstrap / singleton ownership
2. request receipt / validation / dispatch / reply
3. session / connection lifecycle
4. graceful shutdown / drain
5. advisory-stream lifecycle, if it remains daemon-owned

Any SQLite-specific control flow that creates additional daemon machines is a
violation and should be deleted or moved out of the crate.

## Phase Entry Criteria

`Phase AA` begins only when:
- the boundary audit is frozen with the direct daemon-side SQLite leak list
- the user-approved target architecture is recorded in requirements and
  architecture docs
- the new `atm-runtime` crate is accepted as the concrete composition home
- boundary TOML changes are treated as architecture changes rather than
  routine lint data edits

## Sprint Sequence

### AA.0 Daemon Architecture Restatement And State-Machine Inventory

Purpose:
- freeze the exact daemon-side SQLite leak inventory
- restate the intended thin-daemon role in the governing docs before code
  deletion begins
- document the small allowed post-AA daemon state-machine inventory
- freeze the delete / move / keep-and-rewrite classifications that later
  sprints must follow

Execution branch:
- `feature/pAA-s0-daemon-architecture-restatement`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s0-daemon-architecture-restatement`

### AA.1 Subsystem Doctor Traits And Shared Diagnostic Contracts

Purpose:
- define subsystem-owned capability and doctor traits in `atm-core`
- make `atm-rusqlite` the owner of store diagnostics
- allow backend implementations such as SQLite and Claude JSON to satisfy the
  same behavior-named trait family
- make daemon doctor an aggregation surface rather than a subsystem analyzer

Execution branch:
- `feature/pAA-s1-subsystem-doctor-traits`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s1-subsystem-doctor-traits`

### AA.2 `atm-runtime` Skeleton And Composition Transfer

Purpose:
- add `crates/atm-runtime`
- move concrete SQLite construction and production runtime wiring there
- make `atm-daemon` receive only storage-neutral runtime inputs
- freeze the end-state runtime boundary early in
  `runtime-composition.toml` while leaving the SQLite TOML relock to `AA.5`

Execution branch:
- `feature/pAA-s2-atm-runtime-composition-transfer`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s2-atm-runtime-composition-transfer`

### AA.3 Direct Doctor Path And Runtime Health Simplification

Purpose:
- restore a direct local doctor path for config/store checks
- keep daemon health as an additive runtime subreport rather than the only
  doctor path
- remove daemon-owned SQLite health probing

Execution branch:
- `feature/pAA-s3-direct-doctor-and-runtime-health-split`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s3-direct-doctor-and-runtime-health-split`

### AA.4 Delete Remaining Daemon SQLite Leaks

Purpose:
- move SQLite observability injection to `atm-runtime` / `atm-rusqlite`
- move or delete daemon-local SQLite replay/store wrappers
- delete daemon-private SQLite test support

Execution branch:
- `feature/pAA-s4-delete-daemon-sqlite-leaks`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s4-delete-daemon-sqlite-leaks`

### AA.5 Boundary Relock And Permanent Enforcement

Purpose:
- relock the `atm-daemon` / `atm-rusqlite` boundary in code and TOML
- add a second architecture-enforcement layer beyond the TOML lint rules
- treat boundary-policy widening as an explicit architecture change
- add a `boundary-guard` QA agent that reviews plans and phase-ending
  reviews for any boundary loosening before closure
- close the temporary `AA.2` to `AA.4` transition window by making the
  SQLite boundary TOMLs agree with `runtime-composition.toml`

Execution branch:
- `feature/pAA-s5-boundary-relock-and-permanent-enforcement`

Execution worktree:
- `../atm-core-worktrees/feature/pAA-s5-boundary-relock-and-permanent-enforcement`

## Out Of Scope

`Phase AA` does not:
- redesign ATM business logic
- replace SQLite with a second adapter in the same phase
- expand the daemon packet surface
- introduce new product features unrelated to removing SQLite knowledge from
  the daemon
- preserve daemon-local helper code solely because it already exists

## Exit Criteria

Authoritative Phase `AA` exit criteria live only in
`docs/phase-AA/readiness.md`.

This overview intentionally does not restate the closure checklist. Any
Phase `AA` closeout review must use `readiness.md` as the sole source of truth
for final branch/phase exit gates.

## Planning Consequences

The cheapest correct implementation path is deletion-first, not compatibility-
preserving refactor-first.

If a daemon-side SQLite-aware behavior is not essential to:
- transport
- routing
- lifecycle
- bounded request handling

then `Phase AA` should prefer deleting it over inventing a new daemon-local
abstraction for it.

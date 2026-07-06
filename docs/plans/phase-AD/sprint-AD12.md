---
id: AD.12
title: Graft Boundary Reset Planning And Contract Tightening
status: planned
branch: feature/pAD-s12-graft-boundary-reset-planning
worktree: ../atm-core-worktrees/feature/pAD-s12-graft-boundary-reset-planning
target: integrate/phase-AD
---

# Sprint AD.12 — Graft Boundary Reset Planning And Contract Tightening

## Goal

- ratify the graft boundary-reset line and produce implementation-ready
  follow-on sprint docs that remove the leaked daemon-owned graft/session model

## Hard Dependencies

- `AD.1` through `AD.11` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`
- `docs/adr/ADR-012-one-message-identity.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-graft/requirements.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/boundaries.md`
- `boundaries/atm-daemon-client/rpc-envelope.toml`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`
- `docs/plans/phase-AD/sprint-AD12.md`
- `docs/plans/phase-AD/sprint-AD13.md`
- `docs/plans/phase-AD/sprint-AD14.md`
- `docs/plans/phase-AD/sprint-AD15.md`
- `docs/plans/phase-AD/sprint-AD16.md`
- `docs/plans/phase-AD/sprint-AD17.md`
- `docs/plans/phase-AD/sprint-AD18.md`
- `docs/plans/phase-AD/sprint-AD19.md`
- `docs/plans/phase-AD/sprint-AD20.md`

## Interfaces To Ratify

The accepted dispatcher boundary after the reset is unary-only:

```rust
pub trait RequestDispatcher: sealed::Sealed + Send + Sync {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}
```

The accepted thin graft client surface remains command-shaped only:

```rust
pub trait AtmGraftClient: Send + Sync {
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;
    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;
    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError>;
}
```

## Paths To Delete

- no implementation paths are deleted in this sprint
- any still-open sprint doc wording that treats daemon-owned graft advisory
  register/unregister/fetch/drain/stream behavior as accepted end state must be
  superseded by the new `AD.14` through `AD.20` line

## Deliverables

- the accepted requirements, architecture docs, and `ADR-019` no longer lock
  ATM into daemon-owned graft session/stream behavior
- the accepted message-identity ADR and requirements docs no longer bless UUID
  message ids on retained ATM paths
- `violation-inventory.md` is the authoritative review artifact for the
  boundary-reset drift
- `plan-phase-AD.md` extends the phase through `AD.20` and records the new
  boundary-reset exit gates
- `docs/project-plan.md` records the same `AD.12` through `AD.20` corrective
  line so the global project index does not stop Phase AD at `AD.11`
- `AD.13` defines the ULID-only message-identity closure with exact deletion
  targets, explicit boundary contracts, and validation gates
- `AD.14` through `AD.17` each define one production-ready graft-boundary
  closure with exact deletion targets, explicit boundary contracts, and
  validation gates
- `AD.18` defines the raw CLI runtime-root closure with exact deletion
  targets, explicit runtime-root contracts, and validation gates
- `AD.19` defines the read-mutation output consistency closure with exact
  deletion targets, explicit output invariants, and validation gates
- `AD.20` defines the metadata-backed body-search consistency closure with
  exact deletion targets, explicit selector invariants, and validation gates

## This Sprint Does Not Close

- shared protocol surface deletion
- ULID-only message-identity implementation
- daemon advisory runtime deletion
- `atm-graft` runtime rewrite
- final smoke/readiness verification

## Closure Ownership Split

- `AD.12` owns the planning baseline only: phase plan, project-plan indexing,
  violation inventory, ADR ratification, and sprint-by-sprint ownership
  assignment
- `AD.14` owns the shared advisory boundary deletion in
  `crates/atm-core/src/{boundary/mod.rs,graft.rs,protocol.rs}`,
  `crates/atm-daemon-client/src/wire.rs`,
  `boundaries/atm-daemon-client/rpc-envelope.toml`,
  `crates/atm/src/composition.rs` advisory-trait implementation sections,
  `docs/atm-core/{requirements,boundaries}.md`, and
  `docs/atm-daemon/protocol-icd.md`
- `AD.15` owns daemon runtime deletion plus final closure of
  `docs/atm-daemon/{requirements,architecture,boundaries}.md`
- `AD.16` owns `atm-graft` runtime deletion plus final closure of
  `docs/atm-graft/{requirements,architecture,boundaries}.md`
- `AD.17` owns only final verification, readiness evidence, and phase-close
  documentation updates after `AD.13` through `AD.20` land

## Acceptance Criteria

- `ADR-019`, `docs/atm-core/requirements.md`,
  `ADR-012`, `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/requirements.md`,
  `docs/atm-core/architecture.md`, `docs/atm-daemon/requirements.md`,
  `docs/atm-daemon/architecture.md`, `docs/atm-graft/requirements.md`,
  `docs/atm-graft/architecture.md`, and `docs/atm-graft/boundaries.md` all
  describe the thin receiver boundary
  rather than daemon-owned graft session/stream runtime
- accepted docs state that retained ATM message identity is ULID-only and that
  UUID compatibility was retired with the Claude backend
- no remaining `Phase AD` planning doc claims that daemon-owned graft advisory
  session/register/fetch/drain/stream behavior is the accepted release design
- the phase plan explicitly states that `AD.12` through `AD.20` are required
  to close `Phase AD`
- the ownership split above leaves no shared deletion target or accepted
  requirements/ADR/boundary/protocol doc without one final closing sprint
- each new sprint doc names the exact files, deletion targets, and validation
  commands needed for its closure without relying on downstream prompt
  interpretation
- each new sprint doc explicitly lists every associated accepted
  requirements/ADR/boundary/protocol doc in `Exact Targets`

## Required Validation

- manual review against `.claude/skills/plan-hardening/sprint-planning-guidelines.md`
- `git diff --check`

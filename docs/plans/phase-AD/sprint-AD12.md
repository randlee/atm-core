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
- `docs/atm-core/requirements.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-graft/requirements.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/boundaries.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`
- `docs/plans/phase-AD/sprint-AD12.md`
- `docs/plans/phase-AD/sprint-AD13.md`
- `docs/plans/phase-AD/sprint-AD14.md`
- `docs/plans/phase-AD/sprint-AD15.md`
- `docs/plans/phase-AD/sprint-AD16.md`

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
  superseded by the new `AD.13` through `AD.16` line

## Deliverables

- the accepted requirements, architecture docs, and `ADR-019` no longer lock
  ATM into daemon-owned graft session/stream behavior
- `violation-inventory.md` is the authoritative review artifact for the graft
  boundary drift
- `plan-phase-AD.md` extends the phase through `AD.16` and records the new
  boundary-reset exit gates
- `AD.13` through `AD.16` each define one production-ready closure with exact
  deletion targets, explicit boundary contracts, and validation gates

## This Sprint Does Not Close

- shared protocol surface deletion
- daemon advisory runtime deletion
- `atm-graft` runtime rewrite
- final smoke/readiness verification

## Acceptance Criteria

- `ADR-019`, `docs/atm-core/requirements.md`,
  `docs/atm-daemon/requirements.md`, `docs/atm-daemon/architecture.md`,
  `docs/atm-graft/requirements.md`, `docs/atm-graft/architecture.md`, and
  `docs/atm-graft/boundaries.md` all describe the thin receiver boundary
  rather than daemon-owned graft session/stream runtime
- no remaining `Phase AD` planning doc claims that daemon-owned graft advisory
  session/register/fetch/drain/stream behavior is the accepted release design
- the phase plan explicitly states that `AD.12` through `AD.16` are required
  to close `Phase AD`
- each new sprint doc names the exact files, deletion targets, and validation
  commands needed for its closure without relying on downstream prompt
  interpretation

## Required Validation

- manual review against `.claude/skills/plan-hardening/sprint-planning-guidelines.md`
- `git diff --check`

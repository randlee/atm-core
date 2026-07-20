---
title: AI.4 error consumer migration and enforcement
status: proposed
branch: feature/pAI-s4-error-consumer-migration
worktree: ../atm-core-worktrees/feature/pAI-s4-error-consumer-migration
target: integrate/phase-AI
---

# AI.4 — error consumer migration and enforcement

## Deliverables

1. Migrate the remaining direct error producers (currently 88 construction
   sites across 23 source files) in daemon, client, CLI, graft, storage, and
   tests to the AI.3 contract.
2. Preserve safe CLI/doctor presentation while moving diagnostic detail to
   structured boundary logs rather than an alternate transport error shape.
3. Add a mechanical gate that rejects direct ad-hoc construction and duplicate
   code/template mappings outside the approved module.

## Acceptance criteria

- Source inventory reports zero unapproved direct constructors and zero old
  error-field consumers.
- CLI, graft, UDS client, and daemon error tests observe identical JSON/error
  shape for the same code.
- Gate negatives prove both a direct constructor and duplicate mapping fail.

## Required validation

Full workspace tests, CLI/daemon error integration tests, `just lint`, `just
test`, and the error-consumer gate.

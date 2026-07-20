---
title: AI.4 error consumer migration and enforcement
status: complete
branch: feature/pAI-s4-error-consumer-migration
worktree: ../atm-core-worktrees/feature/pAI-s4-error-consumer-migration
target: integrate/phase-AI
---

# AI.4 — error consumer migration and enforcement

## Deliverables

1. Migrate every remaining direct error producer in daemon, client, CLI,
   graft, storage, and tests to the AI.3 contract. Before editing, record the
   current source inventory; closure requires that inventory to be empty except
   for the AI.3-approved constructor module.
2. Preserve safe CLI/doctor presentation while moving diagnostic detail to
   structured boundary logs rather than an alternate transport error shape.
3. Add a mechanical gate that rejects direct ad-hoc construction and duplicate
   code/template mappings outside the approved module.

## Contract

AI.4 adds no application error type. Its gate permits construction only through
AI.3's approved constructor module and fails on retained error branches.

## Acceptance criteria

- Source inventory reports zero unapproved direct constructors and zero old
  error-field consumers.
- CLI, graft, UDS client, and daemon error tests observe identical JSON/error
  shape for the same code.
- Gate negatives prove both a direct constructor and duplicate mapping fail.

## Required validation

Full workspace tests, CLI/daemon error integration tests, `just lint`, `just
test`, and the error-consumer gate.

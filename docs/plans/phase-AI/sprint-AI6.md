---
title: AI.6 canonical write path
status: proposed
branch: feature/pAI-s6-canonical-write-path
worktree: ../atm-core-worktrees/feature/pAI-s6-canonical-write-path
target: integrate/phase-AI
---

# AI.6 — canonical write path

## Deliverables

1. Define one `WriteRequest` carrying immutable message data and optional
   `acknowledges_message_id`.
2. Route CLI send/ack, graft, and local UDS REST to one write handler and one
   sealed storage method.
3. Make the handler persist idempotently, apply optional receiver-side ack
   mutation, then emit the post-write event exactly once.
4. Delete duplicate send/ack envelopes, handlers, persistence/nudge branches,
   and host-routing decisions.

## Acceptance criteria

- The REST ack endpoint differs from send only by `acknowledges_message_id` on
  `WriteRequest`.
- One structural call graph reaches storage and post-write emission for all
  write ingress sources.
- Same-message ULID replay is idempotent and does not duplicate a nudge.
- Self-send policy is checked once before write routing; no later special ack
  exception exists.

## Required validation

CLI, graft, and REST send/ack integration tests; duplicate-ULID and failed-write
tests; `just lint`; `just test`; canonical-write architecture gate.

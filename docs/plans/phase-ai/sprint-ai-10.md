---
title: AI.10 proof and closeout
status: proposed
branch: feature/pAI-s10-crosshost-proof-closeout
worktree: ../atm-core-worktrees/feature/pAI-s10-crosshost-proof-closeout
target: integrate/phase-AI
---

# AI.10 — proof and closeout

## Deliverables

1. Automate the full local UDS, own-IP HTTPS, two-Mac, and Windows peer proof
   matrix in the readiness record.
2. Prove bidirectional send and ack, duplicate ULID idempotence, nudge, failed
   remote ack non-mutation, unavailable peer, and mTLS/allowlist rejection.
3. Prove that `agent`, `agent:chat-a`, and `agent:chat-b` remain independent
   through local, own-IP, and two-host send/read/nudge/ack flows.
4. Remove obsolete Phase AG cross-host runbooks/claims and reconcile user,
   developer, doctor, and architecture documentation with ADR-032–037. Delete
   the retired custom-frame ICD and deprecated boundary records if AI.6 has no
   remaining consumer; otherwise list the concrete retained consumer as a
   release blocker rather than preserving a historical fallback.
5. Run final architecture gates and publish one accepted-tip evidence set.
6. Prove daemon startup rejects invalid enabled HTTPS configuration without a
   partial listener, and prove bounded UDS/HTTPS shutdown drains or cancels
   tracked work within the configured deadline.

## Proof record

Every evidence row records the exact commit, command, sender address,
recipient address, transport, result, and artifact path. Raw TCP connection
success is never a message-delivery proof.

## Acceptance criteria

- Every readiness row names command, exact commit, hosts, and result.
- Cross-host success is a remote write acceptance plus receiver-visible message
  and nudge; raw TCP reachability is insufficient.
- A displayed `from` address and every reply/ack `to` address preserve a
  present chat-id exactly; no operator training or separate session command is
  needed to reply.
- No prior custom frame, named-pipe, peer/replay, duplicate write-path, or
  runtime SQLite escape-hatch source remains.

## Required validation

All readiness commands; `just lint`; `just test`; Windows CI; two-Mac and
Windows-host evidence; local/own-IP/two-host chat-identity proof; final
boundary/error/transport/storage gates.

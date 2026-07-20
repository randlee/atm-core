---
title: AI.8 HTTPS peer transport
status: proposed
branch: feature/pAI-s8-https-peer-transport
worktree: ../atm-core-worktrees/feature/pAI-s8-https-peer-transport
target: integrate/phase-AI
---

# AI.8 — HTTPS peer transport

## Deliverables

1. Bind enabled HTTPS listeners and establish outbound HTTPS connections using
   AI.7 certificate/trust records.
2. Enforce mTLS and exact peer identity/fingerprint before passing an inbound
   request to the shared REST router.
3. Make the post-write router the sole destination-host decision point:
   local/current host -> local nudge; other host -> HTTPS message endpoint.
4. Delete any remote replay, retry, receipt, remote-ack, host-specific
   persistence, or inbound-special-handler code encountered during wiring.

## Acceptance criteria

- HTTPS inbound and UDS inbound call the same router and write handler.
- Untrusted/incorrect-fingerprint peers are rejected before routing.
- Unavailable peer returns a normal transport error and adds no transport
  state; a repeated immutable message is the only retry mechanism.
- `localhost` and own-IP use the HTTPS adapter without a special loopback path.

## Required validation

mTLS allow/reject integration tests; two-daemon in-process adapter test; own-IP
HTTPS proof; `just lint`; `just test`; transport-only boundary gate.

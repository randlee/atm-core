---
title: AI.14 Mac peer-pair smoke execution
status: proposed
branch: feature/pAI-s14-mac-peer-smoke
worktree: ../atm-core-worktrees/feature/pAI-s14-mac-peer-smoke
target: integrate/phase-AI
depends_on: AI.13
---

# AI.14 — Mac↔Mac peer smoke execution

Transport invariant: per REQ-CORE-TRANSPORT-001 and ADR-033, Unix hosts
provide HTTP over UDS and loopback TCP; Windows provides loopback TCP only.
All local adapters call the same HTTP router and application handlers.

## Closure

Two physical macOS daemon hosts pass every AI.13 peer-pair case using the
release candidate without code changes or manual database intervention.

## Deliverables

1. Prepare both Macs from the same committed source: matching compatible
   client/daemon releases, persistent singleton daemon, local capability/UDS
   smoke, enabled HTTPS interface, certificate, and exact reciprocal trust.
2. Execute the AI.13 runner in both directions and publish the sanitized
   evidence artifact in the readiness record.
3. Record and resolve any product defect uncovered by the run on its owning
   follow-up sprint; do not relabel a failed row as TCP/environment success.
4. Run AI.13 teardown on both Macs even after a failed case; capture the
   listener/PID cleanup result and do not leave a test daemon running.

## Acceptance criteria

- Bidirectional send, receiver-visible read, and nudge pass.
- Bidirectional acknowledgement preserves chat-qualified source/destination and
  applies state only at the receiving canonical write handler.
- Exact-ULID replay, unavailable peer, failed remote ack, bad certificate, and
  non-allowlisted peer cases satisfy AI.13 expected results.
- Evidence identifies both commits, host roles, listener addresses, and daemon
  versions without exposing credentials or private keys.

## Required validation

AI.13 runner output, `atm doctor --json` on both hosts before/after, sanitized
daemon log windows, and `just lint && just test` at the tested commit.

## Non-closure

AI.14 does not substitute same-host/loopback proof for a physical peer and does
not claim Windows participation.

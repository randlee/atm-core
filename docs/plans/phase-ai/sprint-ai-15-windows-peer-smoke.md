---
title: AI.15 Mac and Windows peer-pair smoke execution
status: proposed
branch: feature/pAI-s15-windows-peer-smoke
worktree: ../atm-core-worktrees/feature/pAI-s15-windows-peer-smoke
target: integrate/phase-AI
depends_on: AI.11, AI.13
---

# AI.15 — Mac↔Windows peer smoke execution

Transport invariant: per REQ-CORE-TRANSPORT-001 and ADR-033, Unix hosts
provide HTTP over UDS and loopback TCP; Windows provides loopback TCP only.
All local adapters call the same HTTP router and application handlers.

## Closure

One physical macOS daemon and one physical Windows daemon pass every AI.13
peer-pair case. Windows proves its loopback-TCP local client path before and
after peer testing.

## Deliverables

1. Build the same release candidate on Windows and macOS; verify Windows local
   CLI/graft loopback-TCP send/read/ack and macOS UDS plus loopback-TCP smoke.
2. Configure enabled HTTPS interfaces, certificates, and reciprocal exact peer
   trust using durable CLI-managed records only.
3. Execute every AI.13 case in both directions and attach evidence to the
   readiness record.
4. Run AI.13 teardown on macOS and Windows even after a failed case; capture
   listener/PID cleanup and do not leave a test daemon running.

## Acceptance criteria

- Windows has no alternate local listener/path in use.
- Mac→Windows and Windows→Mac send/read/nudge/ack pass with the same original
  ULID on both hosts.
- All duplicate, unavailable, failed-ack, mTLS, and allowlist-negative cases
  meet the reusable expected results.
- The runner succeeds without machine-specific code or an environment-based
  peer configuration override.

## Required validation

AI.13 runner output, doctor reports, listener inspection, sanitized logs, and
`just lint && just test` from the exact tested commit on both systems.

## Non-closure

AI.15 is release evidence only. It does not add a Windows-only protocol or
fallback transport.

### Current non-physical validation boundary

The current run validates the macOS build and local HTTPS/mTLS contract only.
The peer-pair runner's configuration and evidence mechanics are exercised with
mock commands only; that result is not ATM peer-pair evidence.

A source audit previously found that the graft post-send notification receiver
still used `interprocess::local_socket` on Windows. That residual has now been
closed: the graft post-send notification channel was migrated to the same
loopback-TCP plus per-bind capability control plane used by the rest of graft's
`send`/`read`/`ack` client transport, removing `interprocess::local_socket`
from `atm-graft` and the CLI on all platforms (not just Windows-gated). A
`atm-graft -> interprocess` `forbidden_edge` in
`boundaries/atm-graft/shared-client-consumer.toml` now blocks any
reintroduction. This closes the last named-pipe surface inside graft; it is
still not evidence that AI.15's physical peer-pair work ran.

Physical execution remains open and is tracked by `AI10-WINDOWS-001` and the
equivalent Mac evidence gap `AI14-QA1-EVIDENCE-GAP`. Closure requires a real
Windows host and a real macOS host running the same release candidate to:

1. build and start their daemons with their normal local transports;
2. configure reciprocal certificate fingerprints and exact trusted-peer
   records;
3. run every AI.13 case in both directions; and
4. capture runner-owned teardown evidence on both hosts.

Do not change this sprint's `status: proposed` until that physical evidence is
attached.

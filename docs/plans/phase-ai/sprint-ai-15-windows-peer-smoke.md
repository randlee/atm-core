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

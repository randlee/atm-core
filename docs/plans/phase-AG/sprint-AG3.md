---
id: AG.3
title: Daemon Loopback Self-Test Surface
status: accepted
branch: feature/cross-host-communication
worktree: ../atm-core-worktrees/feature/cross-host-communication
target: develop
---

# Sprint AG.3 — Daemon Loopback Self-Test Surface

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.3
worktree: ../atm-core-worktrees/feature/cross-host-communication
branch: feature/cross-host-communication
status: accepted
estimated_scope: medium
```

## Goal

Capture the daemon loopback self-test surface that was added during early AG
execution:

- `atm send loopback@localhost ...`
- `atm send loopback@127.0.0.1 ...`

This sprint is not proof of remote host-to-host communication. It is a local
diagnostic mode that exercises the same daemon peer listener/send path and is
useful for:

- proving that daemon peer binding works locally
- proving that actual ATM payload delivery through the peer listener works
- giving Windows/macOS operators a lower-friction diagnostic lane before
  involving firewall, VPN, routing, or second-host coordination

This legitimate local self-test surface is distinct from the separately tracked
client-trusted `peer_loopback_delivery` wire-field bypass design issue folded
into `AG-FIND-004`; proving same-host delivery through `localhost` or self-IP
does not excuse trusting remote peers to assert same-host provenance.

## Deliverables

- documented same-host remote-target proof as a retained product requirement
- localhost/self-IP smoke coverage and operator evidence shape
- explicit plan authority for using ordinary host routing rather than a
  special loopback workaround

## Required Validation

- local same-host validation rows and operator smoke evidence

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- localhost and self-IP same-host proof are explicitly authorized as part of
  the product contract
- same-host proof is documented as an ordinary remote-target surface, not
  misrepresented as proof of separate-host cross-host closure
- the sprint closes the plan/documentation gap around the already-written
  same-host proof code

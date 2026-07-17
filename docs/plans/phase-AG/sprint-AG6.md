---
id: AG.6
title: Doctor Visibility For The Cross-Host Control Plane
status: planned
branch: feature/pAG-s6-doctor-visibility
worktree: ../atm-core-worktrees/feature/pAG-s6-doctor-visibility
target: develop
---

# Sprint AG.6 — Doctor Visibility For The Cross-Host Control Plane

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.6
worktree: ../atm-core-worktrees/feature/pAG-s6-doctor-visibility
branch: feature/pAG-s6-doctor-visibility
status: planned
estimated_scope: medium
```

## Goal

After the AG.4 / AG.5 control-plane work lands, define the `atm doctor`
projection for that control plane.

This sprint converts the new durable control-plane state into an operator-safe
diagnostic surface:

- `atm doctor` must show the configured/bound cross-host interfaces
- `atm doctor` must show host allowlist population/enforcement state
- AG.7 host-pair validation depends on this surface, but that closure stays in
  a separate sprint

## Deliverables

- exact `atm doctor` output contract for:
  - configured listener/bind interface rows
  - currently bound/reachable cross-host endpoints
  - allowlist enabled/disabled state
  - degraded/misconfigured listener warnings
- requirements/architecture wording aligned to the doctor-visible control-plane
  state
- integration-test contract for doctor/runtime projection
- smoke-test contract for same-host doctor + loopback preflight

## Doctor Contract

- `atm doctor` must report the configured cross-host interface rows from AG.4
- `atm doctor` must report whether the daemon actually bound each enabled row
- if a configured row did not bind, `atm doctor` must name the row and the bind
  failure reason
- `atm doctor` must report:
  - whether inbound host allowlist enforcement is enabled
  - whether the allowlist is empty while enforcement is enabled
  - the exact allowed host entries currently active
- `atm doctor` must tell the operator which command to run next when no usable
  listener/bind configuration exists

Illustrative output shape:

```json
{
  "cross_host": {
    "interfaces": [
      {
        "interface_name": "en0",
        "bind_addr": "10.10.0.15",
        "advertise_addr": "10.10.0.15",
        "port": 43101,
        "enabled": true,
        "last_bound_at": "2026-07-15T20:00:00Z",
        "last_bind_error": null,
        "stale_at": null
      }
    ],
    "allowlist": {
      "enforced": true,
      "hosts": [
        { "host_name": "windows-dev-1", "enabled": true }
      ]
    }
  }
}
```

## Required Validation

- doctor output is reviewed only after AG.4 and AG.5 land
- the loopback lane from AG.3 remains a prerequisite diagnostic input
- requirements/architecture diff review proving doctor/output language matches
  the actual AG.4 / AG.5 product model

## Unit-Test Plan

- doctor projection corner cases:
  - no interface rows configured
  - one enabled row bound successfully
  - one enabled row failed to bind
  - one stale row remains visible
  - allowlist enforced but empty
  - allowlist enforced with one enabled and one disabled host

## Integration-Test Plan

- doctor/runtime integration tests proving:
  - SQLite interface rows project into doctor output
  - SQLite allowlist rows project into doctor output
  - bind failures surface through doctor
  - empty enforced allowlist surfaces warning/error state explicitly

## Smoke-Test Plan

- local preflight smoke:
  - `localhost` and self-IP same-host remote-target delivery still work after
    AG.4 / AG.5
- copied-state and real host-pair validation remain explicitly deferred to
  later sprints

## Entry Gate

- `AG.4` and `AG.5` must already define the durable interface and allowlist
  product surface

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- `atm doctor` output is concrete enough for a dev sprint to implement directly
- real host-pair validation is explicitly deferred to AG.7 so this sprint does
  not silently claim both diagnostics and network closure
- the sprint text states clearly that firewall/routing/VPN issues are later
  integration findings, not reasons to add more transport hacks

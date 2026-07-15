---
id: AG.7
title: Cross-Host Listener Allowlist Enforcement
status: planned
branch: plan/phase-ag-multihost-advertise-allowlist
worktree: ../atm-core-worktrees/plan/phase-ag-multihost-advertise-allowlist
target: develop
---

# Sprint AG.7 — Cross-Host Listener Allowlist Enforcement

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.7
worktree: ../atm-core-worktrees/plan/phase-ag-multihost-advertise-allowlist
branch: plan/phase-ag-multihost-advertise-allowlist
status: planned
estimated_scope: medium
```

## Goal

Design the deny-by-default host allowlist that gates inbound cross-host daemon
connections before they can mutate mailbox state.

This is a new runtime enforcement capability, not a documentation cleanup. It
requires persistent allowlist state, handshake-time hostname evaluation,
explicit rejection behavior, and operator-visible diagnostics for refused
connections.

## Deliverables

- schema and policy contract for host-based inbound allowlisting
- exact deny-by-default rule for every inbound cross-host connection
- exact-hostname-only matching rule with no wildcard support
- acceptance criteria for future implementation and verification

## Schema Design

Draft DDL:

```sql
CREATE TABLE daemon_host_allowlist (
    host_name TEXT PRIMARY KEY,
    added_at TEXT NOT NULL,
    added_by TEXT NOT NULL,
    disabled_at TEXT,
    note TEXT
);

CREATE INDEX idx_daemon_host_allowlist_enabled
ON daemon_host_allowlist (disabled_at, host_name);
```

## Enforcement Contract

- inbound cross-host connections are denied unless the remote host presents a
  hostname that matches one enabled allowlist row exactly
- wildcard matching is forbidden
- prefix, suffix, glob, regex, and subnet-derived trust are forbidden
- disabled rows remain stored for auditability but do not authorize access
- hostname comparison uses one canonical lowercase hostname string on both the
  presented peer value and the stored allowlist row; after canonicalization,
  matching is still exact-string-only
- allowlist rejection happens before any mailbox write, ack mutation, or team
  roster mutation
- every rejection must emit a structured log record containing:
  - presented hostname
  - remote socket address
  - reason for denial
  - whether the failure was missing entry or disabled entry

## Required Validation

- design review that proves no inbound host is trusted by default
- proof that the contract covers:
  - unknown host rejection
  - disabled host rejection
  - exact configured host acceptance
  - rejection logging without mutating mailbox state

## Entry Gate

- `AG.6` should already have defined how a host advertises its own reachable
  endpoints, because allowlist enforcement applies to the resulting
  cross-host listener

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the schema is concrete enough for a dev sprint to implement directly
- deny-by-default is explicit and load-bearing, not implied
- no wildcard matching is allowed anywhere in the design
- exact hostname entries are the only authorization mechanism in scope
- the sprint text states clearly that connection-time enforcement is new
  product work, not a small config follow-up
- the design requires rejection before any mailbox or roster mutation occurs

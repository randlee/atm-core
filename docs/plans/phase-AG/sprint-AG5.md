---
id: AG.5
title: Durable Host Allowlist Enforcement
status: planned
branch: feature/pAG-s5-host-allowlist
worktree: ../atm-core-worktrees/feature/pAG-s5-host-allowlist
target: develop
---

# Sprint AG.5 — Durable Host Allowlist Enforcement

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.5
worktree: ../atm-core-worktrees/feature/pAG-s5-host-allowlist
branch: feature/pAG-s5-host-allowlist
status: planned
estimated_scope: medium
```

## Goal

Add the durable deny-by-default inbound authorization surface for cross-host
daemon connections.

This sprint owns the product answer to:

- which remote hosts are allowed to connect
- how those hosts are managed from the CLI
- how the daemon enforces the rule before any mailbox mutation

## Deliverables

- SQLite schema for allowed-host rows
- CLI command surface for managing allowed hosts
- daemon-side exact-hostname enforcement contract
- requirements updates covering inbound host authorization:
  - `docs/requirements.md`
  - `docs/atm-daemon/requirements.md`
- architecture updates covering hostname evaluation, enforcement ordering, and
  rejection logging:
  - `docs/architecture.md`
  - `docs/atm-daemon/architecture.md`
- ADR-029 defining the inbound host-authorization policy:
  - deny-by-default
  - exact-hostname-only matching
  - enforcement before any mailbox mutation
  - interaction with loopback and future transport security
- explicit reconciliation with `AG-FIND-004` and the loopback-bypass design
  issue discovered during PR #556

## Schema Design

Draft DDL:

```sql
CREATE TABLE daemon_allowed_hosts (
    host_name TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)) DEFAULT 1,
    added_by TEXT NOT NULL,
    added_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    disabled_at TEXT,
    note TEXT
);

CREATE INDEX idx_daemon_allowed_hosts_enabled
ON daemon_allowed_hosts (enabled, host_name);
```

## CLI Contract

The CLI surface is:

- `atm daemon hosts allow <hostname> [--note <text>]`
- `atm daemon hosts deny <hostname>`
- `atm daemon hosts remove <hostname>`
- `atm daemon hosts list [--json]`

Expected behavior:

- `allow` creates or re-enables an exact hostname row
- `deny` disables a row without deleting it
- `remove` deletes the row entirely
- `list` shows enabled/disabled state so operators can tell whether the host is
  merely denied or fully removed

Concrete command rules:

- `allow` is idempotent for an existing disabled row and must re-enable it
- `deny` fails if the host does not exist
- `remove` fails if the host does not exist
- hostname normalization rules must be documented once and reused by all four
  commands

## Enforcement Contract

- inbound cross-host daemon connections are denied unless the remote hostname
  matches one enabled `daemon_allowed_hosts.host_name` row exactly
- wildcard matching is forbidden
- prefix/suffix/glob/regex/subnet-derived trust is forbidden
- comparison is exact-string-only after one canonical lowercase normalization
- rejection happens before:
  - mailbox writes
  - ack/reply mutation
  - team/roster mutation
  - loopback-bypass claim consumption from a remote peer
- every rejection must emit a structured record containing:
  - presented hostname
  - remote socket address
  - reason for rejection
  - whether the host was missing or disabled

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct AllowedHostRow {
    pub host_name: String,
    pub enabled: bool,
    pub added_by: String,
    pub added_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

pub trait AllowedHostStore {
    fn allow_host(&self, command: AllowHostCommand) -> Result<AllowedHostRow, AtmError>;
    fn deny_host(&self, host: Hostname) -> Result<(), AtmError>;
    fn remove_host(&self, host: Hostname) -> Result<(), AtmError>;
    fn list_hosts(&self) -> Result<Vec<AllowedHostRow>, AtmError>;
}

pub trait PeerAuthorizationPolicy {
    fn authorize(&self, presented_host: &Hostname) -> Result<(), AuthorizationError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit
boundary ownership so authorization does not remain an implied detail inside
transport code.

## Required Validation

- schema review proving allow/deny/remove are all representable directly in the
  stored state
- CLI review proving every required host lifecycle is operable from the CLI:
  - allow
  - deny
  - remove
  - list
- daemon review proving rejection happens before any mailbox, ack, or roster
  mutation
- policy review proving hostname matching is exact-only and wildcard-free
- requirements diff review proving the product now names inbound host
  authorization as a first-class cross-host surface
- ADR review proving allowlist enforcement is the chosen functional pre-security
  trust gate for AG

## Unit-Test Plan

- hostname normalization:
  - mixed case input normalizes consistently
  - surrounding whitespace is rejected or normalized once, explicitly
  - empty hostname rejected
- command/store behavior:
  - allow new host
  - allow existing disabled host re-enables it
  - deny existing host disables it
  - remove existing host deletes it
  - deny/remove missing host fail predictably
- authorization corner cases:
  - enabled exact match accepted
  - disabled exact match rejected
  - missing host rejected
  - partial match rejected
  - suffix/prefix/wildcard-like input rejected
- mutation ordering:
  - unauthorized peer never reaches mailbox write path
  - unauthorized peer never consumes loopback-bypass claim

## Integration-Test Plan

- SQLite-backed store tests for allow/deny/remove/list lifecycle
- daemon authorization tests proving:
  - rejection occurs before mailbox mutation
  - rejection occurs before ack/reply mutation
  - rejection logging contains host/socket/reason
- CLI integration tests proving command parsing and JSON rendering for:
  - allow
  - deny
  - remove
  - list

## Smoke-Test Plan

- same-host authorization smoke:
  - empty allowlist with enforcement enabled rejects inbound peer
  - allowed exact host is accepted
  - disabled host is rejected
- cross-host smoke dependency:
  - AG.7 host-pair rows must include unauthorized-host rejection row
    `AG-VAL-003A` before the normal authorized send/read/ack matrix

## Acceptance Criteria

- the schema is concrete enough for a dev sprint to implement directly
- the CLI commands are named and specific rather than implied
- host authorization is deny-by-default and load-bearing
- exact-hostname-only matching is explicit; no wildcards are allowed anywhere
- the sprint text explicitly states that this is the real closure path for the
  earlier unauthenticated peer gap tracked under `AG-FIND-004`

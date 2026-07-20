---
id: AG.4
title: Durable Interface Configuration And Binding
status: planned
branch: feature/pAG-s4-durable-interface-config
worktree: ../atm-core-worktrees/feature/pAG-s4-durable-interface-config
target: develop
---

# Sprint AG.4 — Durable Interface Configuration And Binding

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.4
worktree: ../atm-core-worktrees/feature/pAG-s4-durable-interface-config
branch: feature/pAG-s4-durable-interface-config
status: planned
estimated_scope: medium
```

## Goal

Replace ad hoc environment-variable-driven peer/bind setup with a durable
daemon-owned interface configuration surface stored in SQLite and managed
through the CLI.

This sprint owns the product answer to:

- which network interfaces the daemon may bind for cross-host traffic
- which address/port each configured interface should advertise
- how stale interface rows are detected and reported on roaming hosts

## Deliverables

- SQLite schema for daemon cross-host interface rows
- CLI command surface for managing interface rows
- daemon-side binding/refresh contract for those rows
- requirements updates covering durable interface configuration and removal of
  env-driven peer control as the primary operator path:
  - `docs/requirements.md`
  - `docs/atm-daemon/requirements.md`
- architecture updates covering SQLite-owned interface state, bind lifecycle,
  and daemon/runtime ownership:
  - `docs/architecture.md`
  - `docs/atm-daemon/architecture.md`
- ADR-028 defining the cross-host interface configuration control plane:
  - table ownership
  - CLI ownership
  - bind/refresh/staleness lifecycle
  - why env-driven peer selection is historical/transitional only
- explicit deprecation of env-driven peer selection as the target operator
  model

## Schema Design

Draft DDL:

```sql
CREATE TABLE daemon_peer_interfaces (
    interface_id INTEGER PRIMARY KEY,
    interface_name TEXT NOT NULL,
    bind_addr TEXT NOT NULL,
    advertise_addr TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    interface_kind TEXT NOT NULL CHECK (
        interface_kind IN ('lan', 'vpn', 'loopback', 'other')
    ),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)) DEFAULT 1,
    configured_by TEXT NOT NULL,
    configured_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_observed_at TEXT,
    refresh_deadline_at TEXT,
    stale_at TEXT,
    last_bound_at TEXT,
    last_bind_error TEXT,
    UNIQUE (interface_name, bind_addr, port)
);

CREATE INDEX idx_daemon_peer_interfaces_enabled
ON daemon_peer_interfaces (enabled, interface_kind, interface_name);
```

## CLI Contract

The CLI surface is:

- `atm daemon interfaces add <interface-name> --bind-addr <ip> --advertise-addr <ip> --port <port> --kind <lan|vpn|loopback|other>`
- `atm daemon interfaces update <interface-name> --bind-addr <ip> --advertise-addr <ip> --port <port> [--kind ...]`
- `atm daemon interfaces enable <interface-name> --bind-addr <ip> --port <port>`
- `atm daemon interfaces disable <interface-name> --bind-addr <ip> --port <port>`
- `atm daemon interfaces remove <interface-name> --bind-addr <ip> --port <port>`
- `atm daemon interfaces list [--json]`

Concrete command rules:

- `add` fails if the same `(interface_name, bind_addr, port)` row already
  exists
- `update` fails if no matching row exists
- `enable` and `disable` target exactly one existing row
- `remove` is destructive and must name the exact existing row
- `list` is the authoritative operator view of configured interface state

Expected daemon-side behavior:

- enabled rows are the only rows the daemon may attempt to bind for cross-host
  peer listening
- each enabled row is attempted independently at daemon start and refresh time
- one failing row must not suppress binding attempts for other enabled rows
- stale rows remain visible for diagnosis; they are not silently deleted on the
  first failed refresh

## Binding And Refresh Contract

- the daemon reads interface rows from SQLite at startup
- for each enabled row:
  - bind `bind_addr:port`
  - advertise `advertise_addr:port`
  - record `last_bound_at` on success
  - record `last_bind_error` on failure
- on refresh:
  - `last_observed_at` is updated for still-valid rows
  - `refresh_deadline_at` is advanced
  - unreachable / no-longer-present rows are marked with `stale_at`
- stale rows remain operator-visible until explicitly updated or removed

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct PeerInterfaceRow {
    pub interface_id: i64,
    pub interface_name: String,
    pub bind_addr: IpAddr,
    pub advertise_addr: IpAddr,
    pub port: u16,
    pub interface_kind: PeerInterfaceKind,
    pub enabled: bool,
    pub configured_by: String,
    pub configured_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_observed_at: Option<DateTime<Utc>>,
    pub refresh_deadline_at: Option<DateTime<Utc>>,
    pub stale_at: Option<DateTime<Utc>>,
    pub last_bound_at: Option<DateTime<Utc>>,
    pub last_bind_error: Option<String>,
}

pub enum PeerInterfaceKind {
    Lan,
    Vpn,
    Loopback,
    Other,
}

pub trait PeerInterfaceConfigStore {
    fn add_interface(&self, command: AddPeerInterfaceCommand) -> Result<PeerInterfaceRow, AtmError>;
    fn update_interface(&self, command: UpdatePeerInterfaceCommand) -> Result<PeerInterfaceRow, AtmError>;
    fn set_interface_enabled(&self, key: PeerInterfaceKey, enabled: bool) -> Result<(), AtmError>;
    fn remove_interface(&self, key: PeerInterfaceKey) -> Result<(), AtmError>;
    fn list_interfaces(&self) -> Result<Vec<PeerInterfaceRow>, AtmError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit
ownership boundaries so implementation does not invent a hidden env/config
shortcut later.

## Required Validation

- schema review proving one row can represent one bind/advertise surface
  directly without hidden env dependence
- CLI review proving every row lifecycle is operable from the CLI:
  - add
  - update
  - enable
  - disable
  - remove
  - list
- daemon review proving no enabled rows means no cross-host listener bind
- roaming-host review proving stale rows remain diagnosable instead of silently
  disappearing
- requirements diff review proving the product no longer treats
  `ATM_DAEMON_PEER_ADDR` as the intended steady-state operator contract
- ADR review proving the bind/refresh/staleness decision is explicit rather
  than scattered across sprint prose

## Unit-Test Plan

- row-key parsing and validation:
  - invalid port `0`
  - invalid port `65536`
  - invalid IP literal
  - duplicate `(interface_name, bind_addr, port)` rejection
- lifecycle behavior:
  - add -> list includes row
  - update changes only the targeted row
  - disable prevents selection for bind attempts
  - remove deletes only the targeted row
  - stale row remains listable after refresh failure
- refresh corner cases:
  - LAN row disappears while VPN row remains
  - bind succeeds on one enabled row and fails on another
  - no enabled rows produces an empty bind set without panic

## Integration-Test Plan

- SQLite-backed store tests for CRUD + enable/disable semantics
- daemon/runtime composition tests proving:
  - enabled rows are loaded from SQLite
  - one bad row does not suppress other binds
  - bind failures are surfaced in diagnostic state
- CLI integration tests proving command parsing and JSON rendering for:
  - add
  - update
  - enable
  - disable
  - remove
  - list

## Smoke-Test Plan

- same-host smoke:
  - configure one LAN row and verify daemon binds it
  - configure two rows and verify partial bind failure remains diagnosable
- cross-host prep smoke:
  - one host with only LAN rows
  - one host with both LAN and VPN rows
  - verify the configuration is visible before any send/read cross-host rows

## Acceptance Criteria

- the schema is concrete enough for a dev sprint to implement directly
- the CLI commands are named and specific rather than implied
- the daemon bind contract is deny-by-default with respect to interfaces: if no
  enabled rows exist, no cross-host listener binds
- environment variables are documented as transitional/historical only, not the
  desired steady-state operator surface
- staleness/refresh handling for roaming hosts is explicit

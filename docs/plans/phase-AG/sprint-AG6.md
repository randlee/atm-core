---
id: AG.6
title: Multi-Endpoint Advertisement And Staleness Lifecycle
status: planned
branch: plan/phase-ag-multihost-advertise-allowlist
worktree: ../atm-core-worktrees/plan/phase-ag-multihost-advertise-allowlist
target: develop
---

# Sprint AG.6 — Multi-Endpoint Advertisement And Staleness Lifecycle

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.6
worktree: ../atm-core-worktrees/plan/phase-ag-multihost-advertise-allowlist
branch: plan/phase-ag-multihost-advertise-allowlist
status: planned
estimated_scope: medium
```

## Goal

Design the first real replacement for the single static
`ATM_DAEMON_PEER_ADDR` model by defining a SQLite-backed advertised-endpoints
surface that lets one host publish multiple concurrently reachable daemon
addresses and retire stale paths as network conditions change.

This is a new runtime capability, not a config tweak. It requires runtime
interface enumeration, durable advertisement state, refresh scheduling, stale
endpoint withdrawal, and explicit operator-facing failure behavior when no
usable advertised path remains.

## Deliverables

- schema and lifecycle contract for a daemon-advertised endpoint table
- explicit refresh, withdraw, and expiry rules for roaming hosts
- exact ownership boundary for who writes rows and who consumes them
- acceptance criteria for future implementation and operator verification

## Schema Design

Draft DDL:

```sql
CREATE TABLE daemon_advertised_endpoints (
    endpoint_id INTEGER PRIMARY KEY,
    host_name TEXT NOT NULL,
    interface_name TEXT NOT NULL,
    address TEXT NOT NULL,
    port INTEGER NOT NULL,
    address_family INTEGER NOT NULL CHECK (address_family IN (4, 6)),
    interface_kind TEXT NOT NULL CHECK (
        interface_kind IN ('vpn', 'lan', 'loopback', 'other')
    ),
    advertisement_source TEXT NOT NULL CHECK (
        advertisement_source IN ('runtime_probe', 'operator_override')
    ),
    observed_at TEXT NOT NULL,
    refresh_deadline_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    withdrawn_at TEXT,
    last_error TEXT,
    UNIQUE (host_name, interface_name, address, port)
);

CREATE INDEX idx_daemon_advertised_endpoints_host_live
ON daemon_advertised_endpoints (host_name, withdrawn_at, expires_at);
```

## Lifecycle Contract

- the local daemon is the only writer for its own `host_name` rows
- at daemon start, the cross-host listener binds its configured port and
  performs an initial interface snapshot before the host is considered
  advertisement-ready
- every refresh cycle:
  - currently reachable listener addresses are upserted
  - `observed_at` is set to the current timestamp
  - `refresh_deadline_at` is set to `observed_at + refresh_interval`
  - `expires_at` is set to `observed_at + expiry_interval`
- if an address from the previous snapshot is absent in the new snapshot:
  - it is not deleted immediately
  - `withdrawn_at` is set to the current timestamp
  - the row remains queryable for diagnosis until `expires_at`
- consumers must treat a row as usable only when:
  - `withdrawn_at IS NULL`
  - `expires_at > now()`
- a background reap step may delete expired rows after they are already
  unusable to readers
- operator overrides may add durable rows with
  `advertisement_source='operator_override'`, but they still carry
  `refresh_deadline_at` and `expires_at` so stale overrides remain visible and
  bounded

## Required Validation

- design review against the current `ATM_DAEMON_PEER_ADDR` limitation
- proof that the contract handles:
  - LAN + VPN simultaneous reachability
  - LAN disappearance while VPN remains valid
  - full disconnect/reconnect without leaving immortal stale endpoints

## Entry Gate

- `AG.1` through `AG.5` remain historical validation input for the original
  single-peer-address line
- this sprint begins only as a post-`1.3.1` Phase AG extension; it must not be
  back-framed as part of the original release-validation-only scope

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the schema is concrete enough for a dev sprint to implement directly
- the plan explicitly replaces the single static peer-address assumption with
  multi-endpoint advertisement
- staleness handling is explicit:
  - rows are refreshed on a defined cadence
  - vanished interfaces are marked withdrawn instead of silently disappearing
  - expired rows become unreadable for connection selection before any cleanup
    delete runs
- the sprint text states clearly that runtime enumeration and refresh logic are
  new product work, not a quick config addition

# ATM-Daemon-Client Boundary Inventory

This document records the shared bootstrap and transport-envelope boundaries
owned by `atm-daemon-client`.

`atm-daemon-client` exists to remove duplicated same-host daemon launch helpers
from `atm` and `atm-graft` without creating a Rust dependency on
`atm-daemon`.

## DaemonBootstrapClient

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon-client/daemon-bootstrap.toml](../../boundaries/atm-daemon-client/daemon-bootstrap.toml)

Purpose:
- own the shared same-host bootstrap value types, canonical endpoint/bin
  resolution helpers, and launch gate helpers
- keep `atm` and `atm-graft` aligned on daemon auto-start semantics

Rules:
- `atm-daemon-client` owns `resolve_daemon_local_ipc_endpoint()` and
  `resolve_daemon_bin()` as the shared thin-client bootstrap seam consumed by
  both `atm` and `atm-graft`
- `atm-daemon` and `atm-daemon-bootstrap` are not consumers of this seam;
  after `AC.8`, `atm-daemon-bootstrap` remains only the retained-runtime /
  roster transitional shim and must not reappear in this boundary's consumer
  list
- `atm-daemon-client` may depend on `atm-core` specifically for canonical
  ATM-owned environment/config and daemon-endpoint resolution needed by that
  shared seam; it must not use that edge to acquire runtime assembly or
  concrete storage-backend ownership
- `atm-daemon-client` may own shared same-host connection-setup helpers such as
  `try_connect`, `exchange`, and `unexpected_response` when those helpers are
  extracted to keep `atm` and `atm-graft` aligned
- `atm-daemon-client` must not own daemon request-dispatch semantics or other
  request/response business wiring beyond those shared connection-setup helpers
- `atm-daemon-client` must not depend on `atm-daemon` or
  `atm-storage-rusqlite`
- `atm-daemon-client` must not grow daemon business logic or graft-session logic

### Launch environment boundary

`DaemonSupervisor::spawn_daemon` is the single repository-owned shared
auto-start boundary used by both `atm` and `atm-graft`. Immediately before
`Command::spawn`, it removes `ATM_TEAM`, `ATM_IDENTITY`, and
`ATM_ENVIRONMENT` from the child command. The private sanitation helper does
not inspect those values and does not mutate the invoking process environment;
caller identity and team continue to travel in typed request data.

The repository contains no separate OS-native `atm-daemon` service template,
launch script, or installer entry point. The checked-in Hermes launchd plist
is a graft bridge launcher, not a daemon launcher, and therefore does not
replace or bypass this shared boundary. Any future native service launcher
must apply the same three pre-exec removals before daemon execution.

## RpcEnvelope

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon-client/rpc-envelope.toml](../../boundaries/atm-daemon-client/rpc-envelope.toml)

Purpose:
- own the generic transport envelope shared by same-host daemon clients
- keep transport metadata in headers while canonical message and roster bodies
  come from `atm-storage`

Rules:
- `atm-daemon-client` owns `RpcHeader` and `RpcEnvelope`
- the transport envelope may encode and decode canonical shared domain bodies
  from `atm-storage`
- protocol v1 may still carry `RequestEnvelope` / `ResponseEnvelope` values
  inside `RpcEnvelope.body`, but new message or roster body clones must not be
  introduced under that wrapper
- graft-only advisory/session local IPC is not an accepted
  `atm-daemon-client` boundary surface and must not be reintroduced under
  `RpcEnvelope`
- `atm-daemon-client` must not depend on `atm-storage-rusqlite` or any
  retired backend crate
- backend-specific persistence concerns stay below the storage seam and must not
  leak into the transport envelope

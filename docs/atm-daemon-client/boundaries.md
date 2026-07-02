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
- `atm-daemon-client` may own shared same-host connection-setup helpers such as
  `try_connect`, `exchange`, and `unexpected_response` when those helpers are
  extracted to keep `atm` and `atm-graft` aligned
- `atm-daemon-client` must not own daemon request-dispatch semantics or other
  request/response business wiring beyond those shared connection-setup helpers
- `atm-daemon-client` must not depend on `atm-daemon` or
  `atm-storage-rusqlite`
- `atm-daemon-client` must not grow daemon business logic or graft-session logic

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
- `atm-daemon-client` must not depend on `atm-storage-rusqlite` or
  `atm-storage-claude`
- backend-specific persistence concerns stay below the storage seam and must not
  leak into the transport envelope

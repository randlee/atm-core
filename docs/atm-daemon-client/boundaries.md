# ATM-Daemon-Client Boundary Inventory

This document records the shared bootstrap boundary owned by
`atm-daemon-client`.

`atm-daemon-client` exists to remove duplicated same-host daemon launch helpers
from `atm` and `atm-graft` without creating a Rust dependency on
`atm-daemon`.

## DaemonBootstrapClient

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon-client/daemon-bootstrap.toml](../../boundaries/atm-daemon-client/daemon-bootstrap.toml)

Purpose:
- own the shared same-host bootstrap value types and launch gate helpers
- keep `atm` and `atm-graft` aligned on daemon auto-start semantics

Rules:
- `atm-daemon-client` must not own request/response transport wiring
- `atm-daemon-client` must not depend on `atm-daemon` or `atm-rusqlite`
- `atm-daemon-client` must not grow daemon business logic or graft-session logic

# ATM Boundary Inventory

This document captures CLI-owned concrete adapters for Phase R.

Interpretation note:
- `allowed_dependents: []` means no external crate should depend on the CLI's
  private concrete adapters

Canonical machine-readable boundary source:
- [`boundaries/atm/local-socket-client-transport.toml`](../../boundaries/atm/local-socket-client-transport.toml)

## LocalIpcClientTransportAdapter

Purpose:
- Owns the CLI-local implementation of the ClientTransport contract.

Notes:
- The CLI stays thin: parse, map request, call transport, render response.
- The CLI-local transport must use the shared framed ATM packet helpers.
- `atm send --stdin` is resolved before this adapter boundary: daemon-bound
  request DTOs may carry inline bytes or the retained file contract only, never
  a deferred stdin-read marker.

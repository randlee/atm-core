# ATM Boundary Inventory

This document captures CLI-owned concrete adapters for Phase R.

Interpretation note:
- `allowed_dependents: []` means no external crate should depend on the CLI's
  private concrete adapters

Canonical machine-readable boundary source:
- [`boundaries/atm/local-socket-client-transport.toml`](../../boundaries/atm/local-socket-client-transport.toml)

## LocalSocketClientTransportAdapter

Purpose:
- Owns the CLI-local implementation of the ClientTransport contract.

Notes:
- The CLI stays thin: parse, map request, call transport, render response.

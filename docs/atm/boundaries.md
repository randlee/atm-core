# ATM Boundary Inventory

This document captures CLI-owned concrete adapters. Phase-R custom-frame
records are historical through AI.5; Phase AI uses the shared HTTP client
contract and daemon HTTP resources.

Interpretation note:
- `allowed_dependents: []` means no external crate should depend on the CLI's
  private concrete adapters

Canonical machine-readable boundary source:
- [`boundaries/atm/local-socket-client-transport.toml`](../../boundaries/atm/local-socket-client-transport.toml)

## LocalIpcClientTransportAdapter (historical through AI.5)

Purpose:
- Historically owned the CLI-local custom-frame client contract.

Notes:
- The CLI stays thin: parse, map a route-specific HTTP request, call the shared
  HTTP client, and render the response.
- Unix uses UDS or loopback TCP; Windows uses loopback TCP; peers use HTTPS.
  These are HTTP adapters to one router, not separate client contracts.
- `atm send --stdin` is resolved before this adapter boundary: daemon-bound
  request DTOs may carry inline bytes or the retained file contract only, never
  a deferred stdin-read marker.

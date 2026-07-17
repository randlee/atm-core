# AG.11 Completion Report

Status: candidate complete pending final branch validation and merge-forward

## Scope Closed

- remote-target sends now branch on one typed `remote_host` contract
- production dispatch installs one composition-root-owned
  `DaemonCrossHostDelivery`
- host resolution, port selection, and peer-transport invocation no longer
  live in `runtime_health`
- composition no longer certifies or uses the legacy
  `[daemon].peer_listen_addr` listener fallback

## Paths To Delete Or Reduce Ledger

| Ledger item | Disposition |
| --- | --- |
| env-driven peer endpoint selection as operator contract | reduced: AG.11 production dispatch no longer consults workspace config to decide remote routing; routing uses typed `remote_host` plus durable interface rows |
| CLI-only loopback compatibility paths that bypass daemon runtime | deferred to AG.12-AG.14; AG.11 keeps the daemon-owned loopback proof already on branch and does not add any new bypass path |
| cross-host parsing/classification logic in general runtime code | reduced: runtime dispatch now only chooses local vs remote; host resolution and transport invocation moved behind `DaemonCrossHostDelivery` |
| composition-root leakage of transport configuration | deleted for listener binding: `refresh_peer_listeners()` now uses only durable interface rows and no legacy `peer_listen_addr` fallback |
| docs/tests implying loopback special-casing or env steady-state routing | reduced on AG.11 branch for daemon boundaries and readiness docs; remaining broader smoke/runbook cleanup stays with later AG validation rows |

## Validation Snapshot

- `cargo test -p atm-daemon --lib --tests`: PASS
- `just lint`: pending on this branch
- `just test`: pending on this branch

## Findings Disposition

- `AG-FIND-005`: corrective implementation landed on this branch; broader
  localhost/self-IP/other-host revalidation remains queued in AG.12-AG.16

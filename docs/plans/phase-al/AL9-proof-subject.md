# AL.9 proof subject and activation record

This is a static provenance record, not evidence that a host has been switched.
It prevents later physical-proof artifacts from silently changing the executable
or selected transport path.

## Pinned subject

| Field | Value |
| --- | --- |
| AL.8 accepted composition input | `9823712030d3d7d90629390f13f5daafa82c6888` (PR #778 merge) |
| AL.9 source-and-runtime proof revision | `9ceb7bee4676cc09cb9b4bfacd56e1fcf3da8612` on `feature/pal-s9-physical-proof-ledger-freeze` |
| TLS disposition | Out of MVP scope; PR #774 (`0c3bc49a`) quarantined the TLS interop crate and removed legacy HTTPS transport. |
| local proof host | `Darwin arm64` |
| local Rust toolchain | `rustc 1.94.1 (aarch64-apple-darwin)` |
| runtime crate | `atm-http-runtime` |
| process entrypoint | `crates/atm-daemon/src/main.rs` |
| bootstrap entrypoint | `atm_daemon_bootstrap::run_replacement_daemon` |
| release operator | pending team-lead authorization |
| host activation | not performed by this record |

## Static route proof

At the pinned source revision, `atm-daemon`'s Tokio `main` awaits only
`atm_daemon_bootstrap::run_replacement_daemon`; it does not call the retained
`atm_daemon::run_daemon_with_observability` entrypoint. The replacement
bootstrap, in turn, creates `StorageAndNudgeRouter` and starts
`HttpRuntime<Configured>`. That runtime owns the Axum canonical router plus
Unix UDS (where supported) and authenticated loopback TCP listeners.

The retained `atm-daemon` library remains source reference for Phase AM, but
is not the serving path selected by the executable above. This statement is
intentionally limited to daemon serving composition: the CLI retains its
approved compatibility dispatch for non-write operations until their canonical
routes are migrated. AL.9's physical matrix must prove the write path only and
must not represent that compatibility dispatch as a shared write client.

## Activation invariant

Before any hard activation, the named release operator must record:

1. the exact source revision and built binary version;
2. the active listener for each enabled adapter;
3. the endpoint-record publisher for loopback TCP; and
4. the rollback command and owner.

Until that record exists, all AL.9 execution is evidence-only and cannot
authorize AM ledger freeze or legacy-source deletion.

## Required follow-up evidence

- Dynamic process proof after an authorized switch.
- Unix UDS, loopback TCP, graft write, direct-failure/no-replay, M5, and
  Windows matrix artifacts at the pinned AL.9 proof revision. The current
  local/static evidence and outstanding physical rows are recorded in
  [AL9-physical-proof-matrix.md](AL9-physical-proof-matrix.md).
- No TLS proof, adapter activation, or AL.7-artifact reuse: those are outside
  MVP scope by the accepted PR #774 disposition.

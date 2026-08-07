# AL.9 proof subject and activation record

This is a static provenance record, not evidence that a host has been switched.
It prevents later physical-proof artifacts from silently changing the executable
or selected transport path.

## Pinned subject

| Field | Value |
| --- | --- |
| AL.8 composition source | `ace038f3d1eda86254ebb82fedd62fff610d35a8` |
| AL.9 branch after merge-forward | `feature/pal-s9-physical-proof-ledger-freeze` |
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
  Windows matrix artifacts at the pinned AL.9 proof revision.
- A disposition for the sprint's same-host TLS row. TLS was previously
  deferred from the MVP, so that row cannot be reported as passed without a
  renewed scope decision.

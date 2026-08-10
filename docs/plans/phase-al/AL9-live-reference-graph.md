# AL.9 live-reference graph — AM.1 input

**Captured at:** `9ceb7bee4676cc09cb9b4bfacd56e1fcf3da8612`.

This is the observed source graph that AM.1 must use to freeze its draft
ledger. It is not itself a deletion authorization and it does not claim a host
has switched.

## Active replacement composition

```text
atm-daemon/src/main.rs
  -> atm_daemon_bootstrap::run_replacement_daemon
      -> DaemonOwnerGuard
      -> assemble_default_runtime (concrete storage selected only here)
      -> ReplacementReceivedHookSelector
      -> StorageAndNudgeRouter
      -> HttpRuntimeBuilder::build().start()
          -> canonical_api_router
          -> ApiRouter dispatch
          -> sealed storage boundary
          -> post-persist MessageReceivedHookEmitter
          -> Unix UDS (where configured) + loopback TCP
          -> one guarded loopback endpoint-record publisher
```

`atm-http-runtime` contains no concrete SQLite/Rusqlite, tmux, graft, legacy
daemon server, raw HTTP framing, peer-only decoder, resend, or replay edge.
`atm-daemon-bootstrap` is the intentional composition boundary and its
`SqliteStorageFactory` dependency is therefore a **retain** item, not a runtime
violation.

## Client call chains

```text
atm CliComposition::send
  -> async_transport.execute(ApiRequest<Write>)
  -> preferred_local_client -> HttpRuntimeClient

atm-graft GraftClient::send_message
  -> async_transport.execute(ApiRequest<Write>)
  -> preferred_local_client -> HttpRuntimeClient
```

Both use the existing `WriteRequest` JSON and canonical `/v1/atm/messages`
codec. `client::tests::direct_connector_failure_performs_exactly_one_exchange`
proves a direct failure creates no additional client exchange or replay work.

## Retained legacy consumers — mandatory AM.1 rows

| Retained path | Current callers | Why still live | Required AM.1 disposition |
| --- | --- | --- | --- |
| `LocalIpcClientTransportAdapter` and `atm_daemon_client::{try_connect, exchange_request}` | `crates/atm/src/composition.rs` bootstrap/probe plus `receive`, `ack`, doctor/reload/admin dispatch | Those non-write operations remain synchronous while their canonical async route migration is unimplemented | Add a topological row for async non-write conversion, then delete the adapter and client symbols only after every compiled caller is removed. |
| `GraftLocalIpcClientTransport` and the same daemon-client functions | `crates/atm-graft/src/lib.rs` probe/read/ack/admin dispatch | Python/graft receiver boundary still consumes synchronous non-write methods | Add a separate row for graft/Python non-write conversion; delete this wrapper after its callers migrate. |
| Frozen `crates/atm-daemon` library source | No active executable edge; `Cargo.toml` makes it reference-only | Historical implementation remains for Phase AM comparison/deletion | Delete only according to AM's frozen topology; do not remodel or reactivate it. |

## Observability, doctor, dashboard, and configuration inventory

| Surface | Observed current consumer | Disposition for AM.1 freeze |
| --- | --- | --- |
| Runtime health/readiness | `atm-http-runtime::RuntimeHealth`, bootstrap, and `StorageAndNudgeRouter::doctor` | Retain; replacement owns it. |
| Doctor routes/runtime ports | `StorageAndNudgeRouter::doctor` through core doctor APIs | Retain; test the replacement response before deletion. |
| CLI bootstrap trace | `atm` and `atm-graft` compositions use daemon-client supervisor/probe | Retain until the synchronous non-write conversion row is complete. |
| Graft receiver configuration/observability | `atm-graft::runtime` and injected received-hook selector | Retain; runtime remains harness-neutral and must not import graft. |
| Legacy peer delivery/replay observability, capacity/state, and replay config | Not in active replacement composition; exact compiled callers still require AM inventory | Draft removal candidates only. AM.1 must enumerate every key/consumer and select retain/remove/migration; no deletion based on this absence alone. |

## Ledger lifecycle

AM.1 may draft rows now. It freezes them only against this graph plus the
accepted AL.9 physical/benchmark evidence. The discovered caller topology,
not AM.2–AM.5 numbering, determines deletion order. If AL.9 remains blocked,
the ledger remains draft and AM deletion cannot begin.

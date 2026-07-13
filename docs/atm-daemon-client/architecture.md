# ATM Daemon Client Architecture

`atm-daemon-client` is the shared thin-client boundary between CLI composition
and the local IPC transport. Its public surface remains limited to bootstrap,
bounded connection/exchange helpers, and RPC framing.

## AF admission flow

```text
CLI input materialization → HostRuntimeScope launch gate → canonical endpoint
    ├─ serving owner: connect and exchange RPC
    └─ no owner: spawn once → daemon owner gate → canonical endpoint bind
```

`HostRuntimeScope`, `HostRuntimeRoot`, `DurableStateRoot`, and
`DaemonAdmissionCode` are shared semantic types rather than raw paths or
strings at the admission boundary. `RpcEnvelope` may carry canonical
`RequestEnvelope`/`ResponseEnvelope` data, but it must not own daemon dispatch
or backend storage. Caller stdin is materialized before this crate receives a
compose request.

The machine-readable contracts are
`boundaries/atm-daemon-client/daemon-bootstrap.toml` and
`boundaries/atm-daemon-client/rpc-envelope.toml`; any AF implementation change
must update these records and their lint gates in the same change.

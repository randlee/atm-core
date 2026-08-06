# AL.4 — Shared Standard HTTP Client

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2. Merge AL.2's pushed integration commit before each
development/fix round; AL.2 PR merge is not required.
**unblocks:** AL.5, AL.6, and AL.7.
**parallel_safe:** AL.3; it has no received-hook/shared-dispatch changes.

**traceability:** `REQ-CORE-TRANSPORT-001/002/004/005`,
`REQ-DAEMON-TRANSPORT-003/004`, ADR-032, ADR-033, ADR-041.

## Deliverables

1. Provide one `HttpRuntimeClient<Connector>` implementation of the existing
   sealed `DaemonApiClient` operation for all callers. Migrate that existing
   trait to `#[async_trait]` so its async `execute` remains usable through its
   retained `Arc<dyn DaemonApiClient>` sites; update every allowlisted
   implementation in the same PR. This is one explicit, framework-backed
   dynamic-dispatch choice—no manual vtable, blocking `block_on` bridge, new
   peer trait, or transport wrapper. `MessageReceivedHookEmitter` remains its
   synchronous object-safe post-persistence boundary.

```rust
async fn execute(
    &self,
    endpoint: &ExistingTypedEndpoint,
    request: &ExistingApplicationRequest,
) -> Result<ExistingApplicationResult, AtmError>;
```

   The symbolic names stand for the AL.1 inventory's existing types; none is a
   new public declaration.

2. Migrate `crates/atm-graft/src/transport.rs` outbound traffic from
   `atm_daemon_client::exchange_request` and `try_connect` to the concrete
   `HttpRuntimeClient<Connector>` operation through the shared asynchronous
   `DaemonApiClient` trait. Preserve graft's independent startup and test
   injection; neither `atm-daemon` nor `atm-http-runtime` imports `atm-graft`.
3. Implement connector-neutral request serialization, `/v1/atm/messages`
   path selection, response decoding, deadline propagation, and outcome
   mapping once. Connector construction is delegated to AL.5–AL.7.
4. Configure request deadline, body/connection limits, cancellation, tracing,
   and drain registration through Tokio/framework facilities—not ATM-owned
   per-connection threads, polling loops, queues, or coordinators.
5. Configure and distinguish the complete client failure set: endpoint/DNS
   resolution, connect/refusal/network reachability, TLS handshake/hostname/
   mTLS authorization, request write, response status/ADR-032 error, response
   decode/protocol, cancellation, runtime shutdown, and timeout. Timeout
   sources are separately bounded by one absolute request budget: DNS/endpoint
   resolution, connect, TLS handshake, request write, response first-byte/
   body read, and the whole client operation. Each cause retains typed context;
   none triggers retry or replay.

## Acceptance criteria

- Test connectors prove future UDS/loopback/TLS callers all call one client
  encode/decode implementation.
- Cross-host direct send issues one ordinary unchanged canonical write to the
  canonical route; it does not schedule retry, replay, batching, or a peer
  request body.
- No automatic retry/replay starts, no `message[]` body is constructed, and
  no manual HTTP parser/writer is introduced.
- `crates/atm-graft/src/transport.rs` no longer imports or calls legacy
  `atm_daemon_client::exchange_request` / `try_connect`; graft and CLI use the
  same concrete shared-client encoding/decoding path.
- Every retained `DaemonApiClient` implementer compiles as an `#[async_trait]`
  implementation, and `MessageReceivedHookEmitter` remains synchronous and
  object-safe; source checks prove no `block_on`, manual future vtable, or
  second client trait was introduced.

## Required validation

- timeout/cancellation test using Tokio time control
- static test that transport client code has no raw framing symbols
- graft outbound migration smoke plus one assertion for every named failure
  cause and timeout source

## Non-closure

No physical listener/client migration or deletion occurs here. AL.5–AL.7 own
adapter activation; AM owns deletion.

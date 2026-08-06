# AL.4 — Shared Standard HTTP Client

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2.
**unblocks:** AL.5, AL.6, and AL.7.
**parallel_safe:** AL.3; it has no received-hook/shared-dispatch changes.

**traceability:** `REQ-CORE-TRANSPORT-001/002/004/005`,
`REQ-DAEMON-TRANSPORT-003/004`, ADR-032, ADR-033, ADR-041.

## Deliverables

1. Provide one implementation of the existing sealed `DaemonApiClient`
   application operation for all callers. It accepts/returns the existing
   application and route types; it creates no peer client trait or transport
   wrapper:

```rust
async fn execute(
    &self,
    endpoint: &ExistingTypedEndpoint,
    request: &ExistingApplicationRequest,
) -> Result<ExistingApplicationResult, AtmError>;
```

   The symbolic names stand for the AL.1 inventory's existing types; none is a
   new public declaration.

2. Implement connector-neutral request serialization, `/v1/atm/messages`
   path selection, response decoding, deadline propagation, and outcome
   mapping once. Connector construction is delegated to AL.5–AL.7.
3. Configure request deadline, body/connection limits, cancellation, tracing,
   and drain registration through Tokio/framework facilities—not ATM-owned
   per-connection threads, polling loops, queues, or coordinators.

## Acceptance criteria

- Test connectors prove future UDS/loopback/TLS callers all call one client
  encode/decode implementation.
- Cross-host direct send issues one ordinary unchanged canonical write to the
  canonical route; it does not schedule retry, replay, batching, or a peer
  request body.
- No automatic retry/replay starts, no `message[]` body is constructed, and
  no manual HTTP parser/writer is introduced.

## Required validation

- timeout/cancellation test using Tokio time control
- static test that transport client code has no raw framing symbols

## Non-closure

No physical listener/client migration or deletion occurs here. AL.5–AL.7 own
adapter activation; AM owns deletion.

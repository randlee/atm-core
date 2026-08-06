# AL.1 — Runtime Contract and Crate Boundary

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** `develop` and archived AK.11 hook source
`88bca9d5e232006339f43a4e97eef335531b8a8f`. AL.1 directly copies or narrowly
cherry-picks its exact hook-boundary file set; it does not require AK completion
or merge and must not take unrelated AK transport/replay/listener changes.
**unblocks:** AL.2 directly; AL.3 and AL.4 only after AL.2's canonical handler
integration commit is available.
**parallel_safe:** none. AM.1's boundary-specific inventory begins only after
this sprint's pushed integration commit is merged forward.

**traceability:** `REQ-CORE-TRANSPORT-001`, `001B`, `005`,
`REQ-CORE-BOUNDARY-001/002`, `REQ-DAEMON-RUNTIME-002`, ADR-001, ADR-032,
ADR-033, ADR-036. See
[`phase-al-am-requirement-adr-traceability.md`](../phase-al-am-requirement-adr-traceability.md).

## Deliverables

1. Add `crates/atm-http-runtime` to the workspace as a library only.
2. Add Tokio, Axum/Hyper, Rustls, and one maintained Tokio client behind
   minimal pinned workspace features.
3. Define a small public composition surface; it accepts the existing core
   contracts and never a SQLite, tmux, graft, CLI, or daemon-bootstrap type.
4. Add compile/dependency boundary tests for the shared checklist.
5. Record the compatibility oracle before writing transport code: for each
   migrated route, identify the exact existing public request body, successful
   result, warning representation, ADR-032 error body/status mapping, OpenAPI
   schema, and serializer entry point. This inventory must demonstrate that
   AL adds **no** transport struct, wrapper, field, or JSON encoding.
6. With the exact archived AK.11 hook copy, update every affected boundary record,
   crate export, and architecture note from `PostSendHookEmitter` to the
   receiver-only `MessageReceivedHookEmitter`; preserve sealing and the
   existing allowlisted implementation topology rather than widening it.
7. Record the archived source commit, source file, exact sealed trait signature,
   and the existing result/disposition method that distinguishes new,
   idempotent duplicate, and conflict. If any one is unavailable, leave AL.1
   blocked for an explicit core-boundary decision; do not add a trait.
8. Capture baseline malformed-JSON, oversized-body, and bad-header HTTP
   response fixtures, and establish whether the existing successful schema can
   represent a hook warning. A missing representation is a start-of-phase API
   decision, not an AL.3 discovery.

```rust
pub struct HttpRuntimeBuilder { /* typed core boundary dependencies */ }

impl HttpRuntimeBuilder {
    pub fn build(self) -> Result<HttpRuntime, AtmError>;
}

pub struct HttpRuntime { /* server/client handles and shutdown */ }
```

The concrete internal fields are deliberately not public. The important
constraint is that the builder receives the existing sealed core boundaries
and not a storage backend implementation.

## Acceptance criteria

- `PostSendHookEmitter` has no active implementation or production reference;
  the exact archived AK.11 `MessageReceivedHookEmitter` contract is reused.
- `atm-http-runtime` compiles without a dependency edge to `atm-storage-rusqlite`,
  `atm-graft`, tmux, `atm-daemon-bootstrap`, or resend code.
- The runtime exports no peer-specific wire type, route, or decoder.
- The inventory proves the runtime uses existing route-specific JSON types;
  `RequestEnvelope`/`ResponseEnvelope` are not exposed as generic wire types.
- The shared boundary checklist is linked from the crate-level docs/tests.
- The disposition and warning-representability checks above are recorded or
  AL.1 is explicitly blocked; no unauthorized core trait/schema change lands.

## Required validation

- `cargo check -p atm-http-runtime`
- architecture/dependency test proving prohibited edges are absent
- focused unit test that constructs the builder with test doubles for core
  boundaries only

## Non-closure

No listener or client migration lands in AL.1. Legacy transport remains live
until AL.9 accepts physical proof and authorizes AM; it is not modified here.

# AL.1 — Runtime Contract and Crate Boundary

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AK.11 hook-contract merge/transplant. AL.1 must begin from a
line where `MessageReceivedHookEmitter` is the only active received-hook trait.
**unblocks:** AL.2, AL.3, AL.4.
**parallel_safe:** AM.1 inventory only; it may not alter live production
transport guards before the AL replacement exists.

## Deliverables

1. Add `crates/atm-http-runtime` to the workspace as a library only.
2. Add Tokio, Axum/Hyper, Rustls, and one maintained Tokio client behind
   minimal pinned workspace features.
3. Define a small public composition surface; it accepts the existing core
   contracts and never a SQLite, tmux, graft, CLI, or daemon-bootstrap type.
4. Add compile/dependency boundary tests for the shared checklist.

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
  the exact AK.11 `MessageReceivedHookEmitter` contract is reused.
- `atm-http-runtime` compiles without a dependency edge to `atm-storage-rusqlite`,
  `atm-graft`, tmux, `atm-daemon-bootstrap`, or resend code.
- The runtime exports no peer-specific wire type, route, or decoder.
- The shared boundary checklist is linked from the crate-level docs/tests.

## Required validation

- `cargo check -p atm-http-runtime`
- architecture/dependency test proving prohibited edges are absent
- focused unit test that constructs the builder with test doubles for core
  boundaries only

## Non-closure

No listener or client migration lands in AL.1. Legacy transport remains live
until AL.5 and is not modified here.

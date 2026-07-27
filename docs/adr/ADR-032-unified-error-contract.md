# ADR-032 — Unified Error Contract

| Field | Value |
| --- | --- |
| ID | ADR-032 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-018, ADR-027, ADR-035, Phase AI |

## Decision

ATM exposes one serializable error value at every protocol boundary:

```rust
pub struct AtmError {
    pub code: AtmErrorCode,
    pub message: String,
}
```

`AtmErrorCode` is the single stable machine-readable classification. Error
templates, constructors, and code-to-message mapping live in one dependency-safe
module. Transport adapters, application handlers, storage adapters, the CLI,
and graft return this value rather than translating it into parallel error
envelopes or kind hierarchies.

`AtmErrorKind`, per-site recovery text, captured sources, captured backtraces,
and `ProtocolErrorEnvelope` are retired from the protocol contract. The one
constructor/catalog module owns safe operator recovery guidance by
`AtmErrorCode`; it renders that guidance into the single `message` field. A
boundary may log structured diagnostic context before returning `AtmError`, but
that context is neither serialized nor exposed through a second accessor.

The stable `AtmErrorCode` vocabulary lives in the dependency-light
`atm-error` crate. Storage and service crates consume and re-export that same
type; neither layer defines a second registry or creates a dependency cycle.

## Required invariants

- One error response schema is used by Unix UDS HTTP, loopback-TCP HTTP,
  HTTPS/TCP, CLI, graft, and tests. HTTP failures use the normal HTTP status
  plus this body; no outer `ResponseEnvelope::Error` wire variant exists.
- A code has one canonical template or explicitly supplied safe detail; a
  second code-to-kind or code-to-text translation table is forbidden.
- Error construction is centralized. Direct ad-hoc construction outside the
  approved constructor module fails the architecture check.
- Error serialization is lossless for `code` and `message` and carries no
  backend-specific type, source, or platform handle.
- Exactly one catalog maps each `AtmErrorCode` to its safe message and recovery
  guidance. A caller may add bounded, non-secret detail only through that
  catalog; it cannot create a second recovery or rendering path.

## Consequences

This is deliberately a two-sprint migration, not a one-sprint rename. The
current source has 88 direct `AtmError` construction sites across 23 source
files and a separate protocol mapping layer. AI.3 replaces the types and
protocol envelope; AI.4 migrates all consumers and installs the mechanical
gate. No caller may retain an old error path after AI.4.

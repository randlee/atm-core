# ADR-032 — Unified Error Contract

| Field | Value |
| --- | --- |
| ID | ADR-032 |
| Status | Proposed |
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

`AtmErrorKind`, recovery text, captured sources, captured backtraces, and
`ProtocolErrorEnvelope` are retired from the protocol contract. A boundary may
log structured diagnostic context before returning `AtmError`; that context is
not transported as an alternative error shape.

## Required invariants

- One error response schema is used by UDS HTTP, HTTPS/TCP, CLI, graft, and
  tests.
- A code has one canonical template or explicitly supplied safe detail; a
  second code-to-kind or code-to-text translation table is forbidden.
- Error construction is centralized. Direct ad-hoc construction outside the
  approved constructor module fails the architecture check.
- Error serialization is lossless for `code` and `message` and carries no
  backend-specific type, source, or platform handle.

## Consequences

This is deliberately a two-sprint migration, not a one-sprint rename. The
current source has 88 direct `AtmError` construction sites across 23 source
files and a separate protocol mapping layer. AI.3 replaces the types and
protocol envelope; AI.4 migrates all consumers and installs the mechanical
gate. No caller may retain an old error path after AI.4.

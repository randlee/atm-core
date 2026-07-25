# ADR-027 — Client/Daemon Schema Compatibility

| Field | Value |
| --- | --- |
| ID | ADR-027 |
| Status | **Accepted** |
| Relates to | ADR-026, ADR-033 |

## Decision

Before a client dispatches a write-shaped request over the admitted
`HostRuntimeScope`, it compares an explicit CLI/daemon schema version and the
HTTP API major with the connected daemon's compatibility verdict. Product
release versions are diagnostics only: `atm 1.3.1` may communicate with
`atm-daemon 1.3.2-beta.1` when their schema and HTTP API major are compatible.
A schema or HTTP-major mismatch fails closed with a typed compatibility error;
no write is dispatched.

The client models this ordering with `Connection<Unverified>` and
`Connection<SchemaVerified>`. The former may only perform compatibility
verification. Write dispatch belongs exclusively to the verified state.

During Phase AI, “API version” means ADR-033's HTTP resource contract; it is
not the retired custom-frame wire version.

Doctor reports client invocation identity/team and release separately from the
daemon-process identity/team and release, plus the negotiated schema and HTTP
API versions. This prevents a connected but draining daemon from being
mistaken for a compatible serving daemon.

## Consequences

- Schema compatibility composes with, but never bypasses, ADR-026 singleton
  admission.
- Compatibility does not use post-send hooks, wrapper scripts, or environment
  payload propagation.
- Mixed releases remain usable when the declared compatibility contracts match.
- HTTP minor and patch additions remain backward compatible; HTTP major
  changes are explicit breaking changes.

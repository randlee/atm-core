# ADR-027 — Client/Daemon Version Compatibility

| Field | Value |
| --- | --- |
| ID | ADR-027 |
| Status | **Accepted** |
| Relates to | ADR-026, ADR-033 |

## Decision

Before a client dispatches a write-shaped request over the admitted
`HostRuntimeScope`, it compares its normalized release version and supported
daemon API version with the connected daemon's compatibility verdict. A
mismatch fails closed with `ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE`; the client
must install a matching `atm`/`atm-daemon` pair and no write is dispatched.

The client models this ordering with `Connection<Unverified>` and
`Connection<VersionVerified>`. The former may only perform compatibility
verification. Write dispatch belongs exclusively to the verified state.

During Phase AI, “API version” means ADR-033's HTTP resource contract; it is
not the retired custom-frame wire version.

Doctor reports client invocation identity/team and version separately from the
daemon-process identity/team and version. This prevents a connected but
draining daemon from being mistaken for a compatible serving daemon.

## Consequences

- Version compatibility composes with, but never bypasses, ADR-026 singleton
  admission.
- Compatibility does not use post-send hooks, wrapper scripts, or environment
  payload propagation.
- Mixed releases fail deterministically before durable mutation.

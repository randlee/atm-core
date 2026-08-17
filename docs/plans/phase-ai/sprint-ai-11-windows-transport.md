---
title: AI.11 HTTP contract and Windows local TCP
status: complete
branch: feature/pAI-s11-post-merge-remediation
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pAI-s11-post-merge-remediation
target: integrate/phase-AI
---

# AI.11 — HTTP contract and Windows local TCP

## Closure

The published HTTP contract is real rather than an enum envelope tunneled over
HTTP, and Windows local clients use loopback TCP rather than named pipes or
AF_UNIX. Unix retains UDS and also supports the same loopback-TCP local ingress
for parity testing.

## Deliverables

1. Replace the generic HTTP `RequestEnvelope`/`ResponseEnvelope` body encoding
   with route-specific JSON schemas from `openapi.yaml`. Keep the internal
   `ApiRequest`, `ApiResponse`, and `AtmError` domain types; the HTTP adapter
   alone maps them to HTTP request/response bodies, status, and `Location`.
2. Implement every accepted resource operation: messages list/create,
   message inspect/clear/read/ack, and doctor. `/teams` and `/team/{name}` are
   not part of this sprint because their prior removal is an accepted waiver.
3. Return documented HTTP results: `200` reads/doctor/read mutation, `201`
   create/ack with `Location`, `204` clear, and an HTTP 4xx/5xx status with the
   direct ADR-032 JSON `{code,message}` body for errors. No response body wraps
   errors in a `ResponseEnvelope::Error` variant.
4. On Windows, replace all local named-pipe and AF_UNIX code with HTTP/1.1 over
   loopback TCP. Unix keeps HTTP/UDS and adds supported loopback TCP. Both use
   the same router and route schemas.
5. Add a runtime-local `local-http.json` record beside the existing singleton
   lock. After binding OS-assigned loopback ports, the singleton writes the
   IPv4 endpoint, optional IPv6 endpoint, and a fresh 32-byte base64url
   capability with owner-only filesystem/Windows ACL permissions. Local
   loopback clients send it in `X-ATM-Local-Capability`. No environment
   variable, globally fixed port, or public/LAN local bind is permitted.

   This capability is a transport-swap corequisite, not a new application
   feature: Windows loses UDS owner permissions when it moves to TCP, and a
   loopback address alone does not identify the local owner. The record and
   capability restore the same local-only admission boundary without adding a
   route, handler, storage access, or routing path.
6. Update ADR-033, `docs/requirements.md`, daemon requirements, daemon
   architecture/boundaries, Phase AI plan/readiness, and HTTP API docs so they
   consistently state Unix UDS plus loopback TCP, Windows loopback TCP only,
   and peer HTTPS/mTLS TCP. Delete named-pipe/Windows-AF_UNIX claims.

   This sprint closes triage findings `AI11-BLOCKING-01-WINDOWS-NAMED-PIPE` and
   `AI11-BLOCKING-02-HTTP-CONTRACT-GENERIC`.

## Boundary contract

```rust
pub enum AuthenticatedIngress { Local, Peer }

pub struct LocalHttpEndpointRecord {
    pub schema_version: u8, // 1
    pub daemon_instance_id: Ulid,
    pub ipv4_loopback: Option<SocketAddr>,
    pub ipv6_loopback: Option<SocketAddr>,
    pub capability_base64url: String, // encodes exactly 32 bytes
    pub issued_at: IsoTimestamp,
    pub revoked_at: Option<IsoTimestamp>,
}

// The private HTTP adapter maps `ApiResponse` to status, headers, and a
// route-specific body. It does not expose a generic HTTP response domain type.
```

Adapters authenticate then call `ApiRouter`. They do not decide message
routing from socket type or address, access storage, mutate acknowledgement
state, or emit a nudge. A valid runtime capability yields local ingress; exact
mTLS peer authentication yields peer ingress; every other connection is
rejected before routing. `local-http.json` is owner-readable only. A client
rejects a record whose instance ID does not match the singleton owner, whose
capability does not decode to 32 bytes, or whose `revoked_at` is present; an
orderly shutdown writes revocation before endpoint removal.

## Deletion inventory

- `platform_local_ipc_endpoint_path` Windows named-pipe mapping;
- all `\\\\.\\pipe\\` construction, handling, wake behavior, tests, and docs;
- all Windows AF_UNIX local-client/listener branches;
- named-pipe listener connection-drain and force-cancel-deadline tests only
  after their assertions are ported to the loopback-TCP listener; the category
  remains required and must not be deleted as transport-specific test loss;
- generic HTTP serialization/deserialization of `RequestEnvelope` and
  `ResponseEnvelope` as wire bodies.

Renaming or retaining either transport as a fallback fails this sprint.

## Acceptance criteria

- Windows CLI and graft send/read/ack use loopback TCP against a persistent
  daemon; no local listener is AF_UNIX, named pipe, or non-loopback TCP.
- Unix CLI/graft prove equivalent UDS and loopback-TCP request fixtures.
- Correct local capability succeeds; missing, invalid, stale, or revoked
  capability is rejected before router/storage/nudge work.
- Each OpenAPI path uses its declared request/response schema and status. A
  documented error is exactly `{code,message}` and not an outer enum.
- Production source, manifests, generated artifacts, and docs contain no
  Windows named-pipe or AF_UNIX local transport support.
- `just lint`, `just test`, Windows CI, OpenAPI additions-only gate, and CLI
  additions-only gate pass.

## Required tests

| Level | Proof |
| --- | --- |
| HTTP contract | every accepted route; `201`/`204`; direct JSON error body; route/body mismatch rejection; `Location` on create/ack. |
| Local TCP | Windows and Unix loopback send/read/ack through persistent daemon; invalid capability is rejected before router. |
| Unix parity | one fixture yields the same `ApiRequest`/response via UDS and loopback TCP. |
| Deletion gate | AST/dependency scan rejects pipe, Windows AF_UNIX, generic envelope wire codec, duplicate router, non-loopback bind, and adapter storage/nudge calls. |
| Lifecycle | metadata publish only after bind; shutdown revokes metadata; stale metadata is replaced only after singleton ownership. |
| Drain/cancel parity | loopback-TCP listener proves the migrated connection-drain and force-cancel shutdown-deadline cases formerly covered by the named-pipe listener. |

## Required validation

Run every route contract and local-transport test above on Unix and Windows,
then run `cargo test -p atm-daemon --lib` and
`cargo test -p atm --test openapi_surface`, the deletion gate, OpenAPI/CLI
additions-only gates, `just lint`, and `just test`.

## Non-closure

AI.11 does not alter canonical write ordering, peer reconciliation, or execute
physical two-host smoke. AI.12 owns post-write routing; AI.13–AI.15 own peer
smoke; AI.16 owns offline reconciliation.

---
title: Sprint AI.23 — One HTTP write endpoint for local, loopback, and peer traffic
---

# Sprint AI.23 — One HTTP write endpoint for local, loopback, and peer traffic

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.23
worktree: ../atm-core-worktrees/feature/pAI-s23-crosshost-shared-write-path
branch: feature/pAI-s23-crosshost-shared-write-path
status: complete
estimated_scope: one canonical write endpoint, ingress convergence, and structural enforcement
```

## Goal

Make `atm send` and `atm ack` from a local CLI, a host-qualified same-host
destination, and a remote daemon use one HTTP write resource, one canonical
`WriteRequest`, one `ApiRouter::route` call, one dispatcher, one persistence
method, and one `PostWriteRouter`. A listener may authenticate an inbound TCP
stream, but it may not select a separate application endpoint or write/nudge
implementation.

## Scope Summary

The local client adapter and HTTPS peer adapter both decode the same HTTP
request schema. The local adapter supplies local connection provenance; the
peer adapter first authenticates the TCP stream, records source provenance,
and consumes the already-used destination host so the receiver does not
re-forward. Both then invoke the same API router and canonical write pipeline.
`localhost` remains valid grammar; the required same-host route proof uses the
daemon's advertised/bound virtual-Ethernet IP over TCP. A host-qualified CLI
write therefore uses the peer TCP listener and its exact HTTP resource; it
cannot fall back to a UDS-only or local-mailbox protocol.

## Governing Requirements

- `REQ-CORE-TRANSPORT-001`: CLI, graft, local HTTP, and peer HTTP use the
  same typed request/response schema.
- `REQ-CORE-TRANSPORT-002`: every present destination host selects the one
  HTTPS transport adapter; persistence precedes one post-write route.
- `REQ-CORE-TRANSPORT-002C`: own advertised/bound IP is an ordinary remote
  target and exercises the peer TCP path, not a loopback shortcut.
- `REQ-CORE-TRANSPORT-003`: no peer-specific write state, ACK state, queue,
  receipt, replay store, or nudge implementation.

## Governing ADRs

- `ADR-033-http-endpoint-contract.md`
- `ADR-034-minimal-cross-host-https-transport.md`
- `ADR-035-canonical-write-ingress-and-host-routing.md`

## Governing Boundaries

- `ApiRouter::route(ApiRequest, AuthenticatedIngress, RequestDeadline)` is
  the sole application ingress. `AuthenticatedIngress` carries only
  connection provenance; it cannot select a different write handler.
- `DaemonRequestDispatcher::dispatch(RequestEnvelope::Write)` is the sole
  write dispatch. `route_write` owns persistence followed by exactly one
  `PostWriteRouter::dispatch`.
- `PostWriteRouter` alone consumes destination host: empty after authenticated
  receipt means local nudge; present at the origin means peer HTTP delivery.
- An ACK is `WriteRequest { acknowledges_message_id: Some(..), .. }`, not an
  alternate request envelope, transport, persistence method, or nudge path.

## Explicit Code Samples

```rust
pub enum ApiRequest {
    Write(Box<WriteRequest>),
    // query variants omitted
}

pub trait ApiRouter: sealed::Sealed + Send + Sync {
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError>;
}
```

`AuthenticatedIngress` is provenance validated by the adapter; after this
call it cannot select a different write, persistence, ACK, or nudge handler.

## Prerequisites

- AI.11–AI.16 establish the HTTP/TCP and canonical write foundations.
- AI.22 preserves the parsed destination host and applies self-send rejection
  only to exact same agent/team with no host.

## Hard Dependencies

- AI.11–AI.16
- AI.21-pre
- AI.22

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta.23` and prove
  the matching client/daemon value through `atm doctor --json` before runtime
  evidence.

## Non-Goals

- Changing peer authority, DNS resolution, mTLS policy, timeout policy,
  reconciliation, or physical multi-host evidence.
- Adding a listener, a second daemon, an ACK endpoint, a loopback-only mode,
  an alternate local socket protocol, or a socket-family routing decision.

## Sub-Tasks

1. **Make one write representation the only send/ACK application request.**
   - **Current state:** `ApiRequest::Write`, shared dispatcher routing, and
     `normalize_peer_write_for_local_delivery` already exist on
     `origin/integrate/phase-AI@cb3af95188c1ba685ed93cec0512e7d38fa7f655`.
     The HTTPS adapter is already wired through
     `DaemonRequestDispatcher` in `runtime_health.rs`.
   - **Target state / remaining work:** verify and add regression coverage that
     those existing seams are the only send/ACK representation:
     `ApiRequest::Write(Box<WriteRequest>)` and
     `RequestEnvelope::Write(Box<WriteRequest>)`, with ACK data only in
     `WriteRequest.acknowledges_message_id`. Delete or deprecate any remaining
     peer-facing `SendRequestEnvelope::Compose` /
     `SendRequestEnvelope::Acknowledge` write dispatch rather than wrapping it.
   - Required tests: serialize a normal send and ACK; assert they differ only
     by the ACK reference and ordinary message fields, then decode to the same
     `ApiRequest::Write` variant.
   - Required document update: align ADR-035's canonical write inventory with
     actual type and module names.

2. **Converge local CLI and peer TCP ingress before dispatch.**
   - Development work: update
     `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`,
     `crates/atm-daemon/src/local_tcp_transport.rs`, and
     `crates/atm-daemon/src/https_transport.rs` to decode the same HTTP schema
     and call the same `Arc<dyn ApiRouter>::route`. The HTTPS handler may do
     only TLS/source authentication and
     `normalize_peer_write_for_local_delivery`: attach authenticated source
     provenance and clear the consumed destination host. It must not persist,
     nudge, ACK, choose a second route, or invoke the dispatcher directly.
   - Required tests: send equivalent normal and ACK HTTP bodies through the
     local TCP adapter and peer TCP/HTTPS adapter. Record that both reach the
     same router call and produce the same `WriteRequest` after permitted peer
     normalization.
   - Required boundary update: in
     `crates/atm-architecture/tests/boundary_enforcement.rs`, fail closed if a
     daemon adapter calls a persistence method, `PostWriteRouter`, nudge sink,
     or a second write handler outside the dispatcher.

3. **Make the canonical dispatcher own every write side effect.**
   - Development work: in `crates/atm-daemon/src/runtime_health.rs`, make
     `DaemonRequestDispatcher::dispatch` forward all
     `RequestEnvelope::Write` values to one private `route_write`. `route_write`
     calls one `MessageWriter::write`, then one `PostWriteRouter::dispatch`.
     Do not branch on `AuthenticatedIngress`, source host, loopback status,
     socket family, or ACK type after router validation.
   - Required tests: a local send, peer send, local ACK, and peer ACK each
     produce one persisted record and exactly one post-write event. An already
     delivered remote duplicate remains a storage no-op with no second nudge.
     AI.24 owns the distinct same-store host-qualified receipt case: its
     origin record already exists locally, so the skipped duplicate write must
     continue the inbound recipient nudge and log that disposition.
   - Required document update: update `docs/architecture.md` and
     `docs/atm-core/architecture.md` with the exact convergence point.

4. **Prove own-IP is a regular peer TCP write, not a local shortcut.**
   - Development work: add a daemon-pair integration helper that targets the
     daemon's advertised/bound virtual-Ethernet IPv4 address. The helper must
     enter the peer HTTP listener; it may not call the dispatcher directly or
     route through a UDS-only fixture.
   - Required tests: invoke normal CLI-equivalent send and ACK writes addressed
     to `<self>@<team>.<advertised-ip>`. Assert the peer listener's HTTP
     decoder and `ApiRouter::route(..., Peer, ...)` execute, the receiver can
     read the resulting message/reply by ULID, and the configured recipient
     nudge occurs after persistence. Keep a separate parser-only `localhost`
     row; it cannot satisfy this TCP proof.
   - Required boundary update: add a structural test that `localhost`/own-IP
     are not matched by a dedicated production branch before the peer adapter.

5. **Set the sprint release identity before runtime evidence.**
   - Development work: first commit updates the workspace release metadata for
     every releasable ATM assembly to `1.3.2-beta.23`; never version CLI and
     daemon independently. Update `Cargo.lock` only if Cargo changes it.
   - Required tests: release-build `atm` and `atm-daemon`, run exactly one
     managed daemon, and require `atm doctor --json` to report matching
     `1.3.2-beta.23` client/daemon releases before endpoint proof begins.
   - Required document update: record this target in the Phase AI plan index.

## Split Recommendation

Do not split ingress convergence from the canonical dispatcher: either all
adapters share it or the invariant is false. Split authority, deadline, and
physical peer evidence into their owning later sprints. If this work requires
an ACK-specific handler or a second router, stop and redesign rather than add
an exception.

## Acceptance Criteria

- A local CLI `atm send`/`atm ack`, a loopback/self-IP TCP receipt, and a
  remote host `atm send`/`atm ack` all use the same HTTP write resource and
  decode to `ApiRequest::Write(WriteRequest)` before `ApiRouter::route`.
- The local TCP adapter and the peer TCP/HTTPS adapter invoke the same router
  object, then the same `DaemonRequestDispatcher::dispatch`, `route_write`,
  persistence method, and `PostWriteRouter`; no adapter has a write, ACK, or
  nudge implementation of its own.
- A host-qualified CLI `atm send`/`atm ack` targets the same TCP HTTP listener
  and HTTP write resource as a remote daemon. UDS is retained for
  unqualified local clients on platforms that support it; it is a byte-stream
  adapter to that resource only and cannot satisfy the advertised-IP route
  proof.
- The advertised/bound virtual-Ethernet IP proof observes
  `ApiRouter::route(..., Peer, ...)`, proves the message/reply ULID in the
  receiver inbox, and proves its nudge after that row is readable. It is not
  satisfied by `localhost`, raw TCP connect, sender-side persistence, or a
  fake transport.
- For send and ACK, peer ingress differs from local ingress only in
  authenticate/attach source provenance and consume destination routing
  metadata before the shared router. No later code branches on ingress, TLS,
  socket family, host locality, or ACK kind.
- Structural tests fail the build if a second HTTP write resource, dispatcher,
  persistence call path, post-write router invocation, or nudge handler is
  introduced.
- `atm doctor --json` reports client and daemon `1.3.2-beta.23` before the
  runtime proof.
- An independent quality review uses the release-built branch daemon and real
  CLI/HTTP traffic to observe the local-TCP and peer-TCP adapters converge at
  the same router, dispatcher, persistence, and post-write events. Unit-only
  router spies cannot close the runtime-evidence acceptance item.

## Required Validation

- `just lint`
- `just test`
- `cargo build --release --bin atm --bin atm-daemon`
- the local-TCP/peer-TCP send-and-ACK convergence suite, with sanitized daemon
  logs, one daemon PID, `atm doctor --json`, and `git diff --check`
- Switch the CLI and daemon together with `daemon-switch`; leave the one
  managed branch daemon running after the evidence suite for quality review.

## Required Document Updates

- `docs/requirements.md` and `docs/atm-core/requirements.md`: name the shared
  HTTP write resource and specify that host-qualified same-host traffic uses
  it.
- `docs/adr/ADR-034-minimal-cross-host-https-transport.md` and
  `docs/adr/ADR-035-canonical-write-ingress-and-host-routing.md`: document
  local/peer adapter convergence and the limited authenticated normalization.
- `docs/architecture.md`, `docs/atm-core/architecture.md`, and
  `docs/plans/phase-ai/{README.md,plan-phase-ai.md}`: record the real owner,
  endpoint, and release candidate.

## Risks And Watchouts

- A TCP listener or TLS handshake alone is not endpoint proof. The test must
  observe the common router, receiver persistence, and receiver nudge.
- Insecure smoke mode may relax peer authentication only; it must still use
  the same TCP HTTP decoder, router, write handler, and post-write router.
- `AuthenticatedIngress` is provenance validation, not an alternate service
  path. Do not let it become a `Peer` write/ACK/nudge branch.

---
title: TLS Transport Replan — Preserve The Plain Direct-Peer Pipeline
status: draft
branch: feature/pao-tls-replan
worktree: ../atm-core-worktrees/feature/pao-tls-replan
target: develop
supersedes_for_planning: phase-ao
---

# TLS Transport Replan

## Decision Context

Phase AO is retained as an audit reference only.  It must not be merged,
reverted, repaired, or used as an implementation base.  Its central design
mistake was routing normal daemon startup through a new TLS-selection and
adapter-availability path.  That changed the working direct-peer pipeline even
when an operator did not want TLS.

This replan starts from `develop` commit `2ed09a5b99eff11b7f64e2270cc1842e71a2f603`.
At that revision the working direct-peer pipeline is:

```text
host-qualified CLI/graft write
  -> atm-http-runtime::selected_write_transport
  -> direct_peer_tcp_client / DirectPeerTcpConnector
  -> existing direct-peer TCP listener
  -> existing canonical Axum router
  -> existing WriteRequest, persistence, post-write hook, and response
```

The normal bootstrap always enables `DirectPeerTcpConfig::standard()` and the
normal direct-peer listener and client have no TLS configuration, storage
lookup, trait object, or alternate startup path.  That is the compatibility
baseline.

## Goal

Add mutually authenticated TLS as an explicit runtime-selected outer stream
adapter while leaving the preceding pipeline behaviorally unchanged whenever
TLS is disabled.  Selecting either transport must require only normal daemon
configuration/restart, never a different build, Cargo feature, benchmark-only
binary, source edit, or environment-only escape hatch.

## Non-Negotiable Invariants

1. **Plain is the compatibility mode.** A normal daemon invocation without an
   explicit TLS selection uses the pre-TLS direct TCP listener and
   `DirectPeerTcpConnector` unchanged.  It must not construct a TLS adapter,
   read a certificate/trust store, branch on adapter availability, change
   listener readiness, or change route/request/response handling.
2. **TLS is opt-in at one I/O seam.** Explicit TLS selection wraps only the
   accepted/connected byte stream before the existing Hyper/Axum HTTP
   processing.  It does not create a second endpoint, request DTO, canonical
   write handler, acknowledgement path, nudge path, persistence path, or
   retry/replay system.
3. **No downgrade.** When TLS is selected, a DNS, configuration, handshake,
   hostname, pin, or peer-certificate failure is reported as TLS delivery
   failure.  It never retries the plain client.  When TLS is not selected,
   no TLS configuration failure is relevant or observable.
4. **Runtime selection, not compilation.** The shipped daemon exposes one
   explicit, documented peer-wire selection with `plaintext` as the default
   and `mtls` as the opt-in value.  Daemon-switch/LaunchAgent arguments are
   the operational control surface.  A restart with another value changes the
   mode; neither mode is a Cargo feature or a special benchmark executable.
5. **Opaque transport ownership.** `peer-tls` is the sole Rustls/Tokio-Rustls,
   certificate, hostname, pin, and `PeerConfigStore` transport consumer.
   `atm-http-runtime` retains the HTTP router/client and sees only an opaque
   selected stream.  `atm-daemon-bootstrap` selects/composes the mode but
   performs no TLS handshake or certificate policy.
6. **Production evidence must use the shipped mode.** Benchmark and smoke
   runs select plain or mTLS by the same normal daemon argument that users use;
   disposable test identities are setup data, never a different runtime path.

## Requirement And ADR Reconciliation Gate

Implementation is blocked until a documentation-only review accepts the
following correction:

- `REQ-CORE-TRANSPORT-002B1` and `REQ-DAEMON-TRANSPORT-002B1` currently say
  normal startup is mTLS and plain is benchmark/debug-only.  That contradicts
  the approved compatibility decision above and must be replaced with explicit
  runtime selection: default `plaintext`, opt-in `mtls`, visible in doctor and
  retained events.
- ADR-034 and ADR-035 continue to require one canonical HTTP application path
  and no plaintext downgrade.  Their transport-mode wording must be amended,
  not silently overridden: `plaintext` is an intentionally selected
  compatibility mode, while `mtls` is the authenticated production mode.
- `docs/adr/INDEX.md` names a missing ADR-047.  The reconciliation sprint must
  either restore the referenced ADR or remove/correct the stale supersession
  claims; it may not cite a nonexistent authority.

The accepted ADR must also state the compatibility invariant and explicitly
reject “optional adapter unavailable => alter the normal listener/client
pipeline” as a design pattern.

## Proposed Delivery Sprints

| Sprint | Closure | Dependencies | Must not do |
| --- | --- | --- | --- |
| TLR.1 — Contract and baseline oracle | Accepted ADR/requirements, explicit daemon mode syntax, and a pre-TLS plain-pipeline characterization suite. | None. | Add TLS crate, listener, or client code. |
| TLR.2 — Isolated TLS stream adapter | `peer-tls` provides inbound/outbound mTLS stream wrapping with focused positive/negative tests and boundary records. | TLR.1 PR merged. | Change normal bootstrap, direct TCP connector, router, or benchmark harness. |
| TLR.3 — Runtime mode seam | One daemon runtime argument selects `plaintext` (existing path) or `mtls` (adapter path); same router/client protocol in both modes. | TLR.2 PR merged. | Infer mode from adapter/config availability or fall back from TLS to plain. |
| TLR.4 — Operational proof and performance | Same shipped binary is benchmarked in both modes on M4, M5, and FastPC4; existing plaintext baseline is compared before TLS is accepted. | TLR.3 PR merged. | Treat functional pass or an absent Windows host as performance success. |

Each child branch must merge its parent before every development or fix round;
parallel work is limited to documentation review that does not modify the same
requirements, ADR, boundary, runtime, or evidence artifacts.

## TLR.1 — Contract And Plain-Pipeline Oracle

### Deliverables

1. An ADR and requirements correction that define the public daemon argument:
   `--peer-wire-security plaintext|mtls`; omitted means `plaintext`.
2. A documented daemon-switch invocation that changes this argument without
   rebuilding either `atm` or `atm-daemon`.
3. A characterization suite for the existing plain pipeline.  It must cover:
   listener bind/port behavior; host-qualified outbound selection; canonical
   `WriteRequest` encoding; router provenance; durable write; hook warning;
   acknowledgement; duplicate behavior; typed direct-connection failure; and
   shutdown/readiness behavior.
4. Structural tests proving that plain-mode startup calls the existing
   `DirectPeerTcpConfig::standard`, `DirectPeerTcpConnector`, and direct-peer
   listener code.  They must fail if plain mode constructs `PeerIoAdapter`,
   calls `peer-tls`, reads `PeerConfigStore`, or introduces a TLS branch below
   the normal startup entrypoint.

### Acceptance Criteria

- The requirements/ADR contradiction is resolved and quality-reviewed before
  TLS code begins.
- The current `develop` plain direct-peer test corpus remains green unchanged;
  new tests add coverage rather than replacing it.
- Starting plain mode with absent, invalid, or conflicting TLS configuration
  produces the same direct-peer readiness and result as no TLS configuration.

### Required Validation

```sh
cargo test -p atm-http-runtime -p atm-daemon-bootstrap
just lint
just test
```

## TLR.2 — Isolated mTLS Stream Adapter

### Deliverables

1. A narrow `peer-tls` implementation that loads existing certificate/trust
   records only after explicit mTLS selection and wraps `TcpStream` inbound and
   outbound.  It owns concrete Rustls/Tokio-Rustls values and typed,
   non-secret failures.
2. A sealed transport-only port or equivalent existing I/O seam, with one
   authorized implementation.  The port accepts/returns byte streams only;
   it exposes no message, roster, acknowledgement, nudge, retry, or storage
   DTO.
3. Boundary records and architecture tests that permit the exact
   `peer-tls -> atm-storage/atm-core` dependencies and forbid Rustls,
   `PeerConfigStore`, and certificate-policy imports in `atm-http-runtime`.
4. Positive mutual-auth stream tests and negatives for missing/disabled
   interface, missing identity, bad key, expired/untrusted client certificate,
   hostname mismatch, pin mismatch, deadline, and handshake failure.

### Acceptance Criteria

- A valid configured pair exchanges opaque bytes over mTLS.
- Every negative fails before a Hyper connection, HTTP decode, router,
  persistence, or receiver hook.
- No code in this sprint is called by the default/plain daemon startup path.

### Required Validation

```sh
cargo test -p peer-tls
just lint
just test
```

## TLR.3 — Runtime Mode Seam

### Deliverables

1. Parse the mode once at daemon launch.  Missing/`plaintext` uses the old
   direct listener and direct client literally; `mtls` composes the adapter at
   bootstrap and wraps only socket accept/connect.
2. Keep the existing HTTP method/path, request encoder/decoder,
   `WriteRequest`, `ApiRouter`, `DaemonRequestDispatcher`, persistence,
   `PostWriteRouter`, response envelope, deadlines, and lifecycle ownership
   common to both modes.
3. Extend doctor and retained observability with the selected mode, never key
   bytes or certificate contents.  Plain mode reports `plaintext`; mTLS
   reports `mtls` and configuration readiness.
4. Tests that invoke the same release daemon interface in both modes.  The
   plain tests run with intentionally invalid TLS state to prove no accidental
   adapter/config dependency; mTLS tests prove no plaintext fallback.
5. Extend TLR.1's structural plain-pipeline guard at this wiring point.  It
   must fail if the `plaintext` launch selection constructs `PeerIoAdapter`,
   calls `peer-tls`, reads `PeerConfigStore`, replaces `DirectPeerTcpConnector`,
   or routes plain traffic through an mTLS/adapter-availability branch.  The
   guard must permit TLS composition only inside the explicit `mtls` arm.

### Acceptance Criteria

- `atm-daemon` built once can be restarted between plain and mTLS modes with
  only its launch argument changed.
- Plain mode retains the TLR.1 behavior oracle exactly.
- The TLR.3 source/architecture guard proves the plain launch arm still
  invokes the original direct-peer listener and client without any TLS
  construction or configuration access.
- mTLS accepts only configured/pinned peers and reaches the same canonical
  write result as plain after authentication.
- A mTLS setup failure leaves local UDS/loopback service available but does
  not silently expose or use plain direct-peer transport.

### Required Validation

```sh
cargo test -p atm-http-runtime -p atm-daemon-bootstrap -p peer-tls
just lint
just test
```

## TLR.4 — Operational And Performance Proof

### Deliverables

1. `just benchmark` runs its existing `tcp` (plaintext) and `tcp-tls` (mTLS)
   targets using the same shipped
   daemon binary and normal `--peer-wire-security` argument.  It records mode,
   binary revision, frames-per-connection, hook mode, platform, hostname, and
   exact baseline provenance.
2. Plain benchmark comparison is mandatory against a compatible pre-TLS
   baseline before TLS is accepted.  No absolute low floor may substitute for
   this comparison.  A material plain regression blocks closure.
3. mTLS measurements are a separate mode and are never compared to plain as a
   functional gate; they still require a same-mode baseline after the first
   accepted campaign.
4. M4, M5, and FastPC4 each run plain and mTLS.  An unavailable platform is an
   explicit blocked artifact, not a pass.  Any temporarily unavailable
   physical-host proof is scoped to TLR.4 rather than blocking TLR.1–TLR.3.
5. Bidirectional M4↔M5 plain and mTLS smoke proves send, read,
   `--requires-ack`, and reply through the ordinary daemon/CLI pair.

### Acceptance Criteria

- Plain throughput is at or above the agreed compatible baseline on the same
  host/mode/profile, or the regression is root-caused and fixed before TLS
  closure.
- mTLS proof includes positive delivery and pre-router negative proof.
- No benchmark uses a feature-gated daemon, a benchmark-only request path, or
  a compile-time transport selection.

### Required Validation

```sh
just benchmark --target tcp
just benchmark --target tcp-tls
just benchmark-report --rebuild
just reports-index --check
just smoke crosshost-send
just test
```

## Explicit Non-Goals

- Reusing, repairing, merging, deleting, or reverting Phase AO.
- Editing the frozen synchronous `crates/atm-daemon` tree.
- A generic TLS framework, new key exchange, certificate rotation/discovery,
  an outbox, replay, retry loop, relay, new controller, or second router.
- Persisting IP aliases or using raw-IP identity in place of hostname plus pin.
- Treating TLS configuration presence as an implicit mode selection.

## Risks And Review Questions

1. The present requirements say default mTLS, while the compatibility decision
   requires default plain.  TLR.1 must resolve this intentionally and make the
   release migration visible; implementation may not choose by implication.
2. Existing direct-peer code couples the outbound client to
   `StorageAndNudgeRouter` for acknowledgement delivery.  TLR.3 must prove
   both ordinary send and ACK use the selected outer transport without moving
   any canonical-write responsibility.
3. The current historical benchmark corpus mixes hardware, transport, hook
   mode, and connection-frame profiles.  TLR.4 must select only exact profile
   matches and make incomparable evidence visible rather than averaging it.

## Review Checklist

- [ ] Requirements and ADR wording name one mode-selection authority and a
      default.
- [ ] The plain-mode source path retains the pre-TLS listener and client.
- [ ] TLS is unreachable from default/plain startup and required for mTLS.
- [ ] Both modes use one HTTP application pipeline.
- [ ] mTLS cannot downgrade to plain.
- [ ] Benchmarks exercise the shipped daemon, compare compatible plaintext
      evidence, and record blocked hosts honestly.

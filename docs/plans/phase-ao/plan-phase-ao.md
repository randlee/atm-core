---
title: Phase AO Plan — Optional mTLS for the Canonical HTTP Peer Path
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AO — Optional mTLS for the Canonical HTTP Peer Path

## Goal

Add opt-in, mutually authenticated TLS to ATM's active Tokio/Axum HTTP peer
path. TLS changes only connection establishment and authenticated peer
provenance. It must not create a second application protocol, handler,
storage mutation path, acknowledgement path, nudge path, retry loop, or
delivery queue.

The existing `atm-peer-tls-interop` crate is retained, tested reference
material for certificate/provisioning fixtures. It is not an active-daemon
dependency and this phase must not simply wire it into the daemon. AO may
selectively move or recreate narrowly required, reviewed TLS helpers at the
runtime boundary while keeping the fixture crate quarantined.

## Why a separate phase

The active direct-peer connector currently uses ordinary HTTP over TCP. The
repository already has durable peer-control-plane records
(`HttpsInterface`, `LocalCertificate`, `TrustedPeer`) and Rustls-backed test
fixtures, but no active TLS listener, mTLS handshake, exact peer identity
binding, or HTTPS connector on the Tokio/Axum runtime line. Treating that as a
configuration switch would silently permit downgrade or identity mistakes.

AO therefore makes TLS a transport option with explicit, fail-closed policy.
The existing canonical request and `ApiRouter` remain unchanged so the risk is
confined to connection/authentication/lifecycle code.

## Entry gate and baseline

Planning may proceed from `develop`. Production implementation begins only
after:

1. the accepted Phase AL runtime is the sole active daemon path;
2. the Phase AM deletion ledger has classified every legacy transport owner;
3. the implementation branch is based on the accepted Tokio/Axum line, not a
   frozen `crates/atm-daemon` source tree; and
4. the active line has the current `PeerConfigStore` control-plane contract or
   a reviewed migration for it.

Every AO implementation PR targets the accepted runtime integration line,
then merges to `develop` only after the normal cross-platform gates. No AO
task may patch, start, test, or restore the frozen legacy daemon.

## Accepted design

| Concern | Decision |
| --- | --- |
| Application protocol | The existing HTTP request/response schema and one `ApiRouter` stay authoritative. TLS wraps the byte stream only. |
| Peer identity | An enabled TLS interface accepts a client only after mTLS succeeds and the verified certificate fingerprint maps to one enabled exact `TrustedPeer` hostname. Claimed host headers, socket IPs, and reverse DNS never establish authority. |
| Server authority | The client validates the configured registered hostname/SNI and the expected pinned server certificate fingerprint before sending an HTTP request. |
| TLS selection | A peer is either explicitly configured for mTLS or for the retained non-TLS direct mode. A TLS-selected peer never falls back to plaintext after DNS, connect, certificate, or handshake failure. |
| Secrets | Durable state keeps only public fingerprints and an opaque private-key reference. Key bytes are loaded by an owner-only provider and never appear in doctor, reports, logs, or database rows. |
| Provenance | The TLS adapter constructs authenticated ingress provenance only after successful mTLS and exact trust lookup. The handler consumes provenance but never parses certificates. |
| Delivery semantics | A transport failure returns the existing typed direct-send failure. AO adds no outbox, durable delivery state, automatic retry, replay, or background sender. |
| Runtime ownership | One enabled TLS interface has one Tokio listener, one lifecycle owner, and one doctor/endpoint publication record. Startup fails closed if configuration, trust, keys, or binding are invalid. |

### Transport configuration model

AO must introduce an explicit internal configuration choice rather than infer
TLS from a port number or a hostname suffix. The final type name is an
implementation decision, but it must be an exhaustive closed enum equivalent
to:

```text
PeerTransportMode = PlainDirect | MutualTls
```

`MutualTls` requires an enabled `HttpsInterface`, a local certificate key
reference, and an enabled exact `TrustedPeer` record. `PlainDirect` remains
the existing explicit compatibility mode during transition. Neither mode may
attempt the other mode on failure. The default and migration policy must be
made explicit in the AO.1 ADR/update; no implicit default may expose a new
network listener.

## Non-negotiable invariants

1. **One write path.** UDS, loopback TCP, plaintext peer TCP, and mTLS peer
   TCP encode the same canonical request and reach the same router, storage
   write, and post-receive hook.
2. **Authentication before dispatch.** An invalid certificate, hostname,
   pin, allowlist entry, client certificate, or disabled peer is rejected
   before body decoding, storage, nudge, or application handler dispatch.
3. **No insecure fallback.** A configured mTLS peer cannot retry as plain
   HTTP, redirect to another authority, or treat an IP address as identity.
4. **No parallel delivery state.** Connection/session state is lifecycle
   state only. There is no retry scheduler, outbox, replay cache, receipt
   store, deferred response, or peer queue.
5. **No legacy repair.** `crates/atm-daemon` remains frozen reference-only.
   All runtime work belongs to `atm-http-runtime` and its active bootstrap.
6. **No fixture coupling.** Production code has no dependency edge to
   `atm-peer-tls-interop` or fixture-only certificate generation code.

## Sprint sequence

| Sprint | Closure | Depends on |
| --- | --- | --- |
| AO.1 | Runtime-facing TLS design and control-plane validation contract, including explicit transport selection and stable error/recovery matrix | accepted AO plan and active Tokio/Axum baseline |
| AO.2 | Fail-closed TLS material loader and configuration validation before any listener binds or endpoint is published | AO.1 |
| AO.3 | Tokio/Axum mTLS listener that creates authenticated provenance and delegates to the existing router | AO.2 |
| AO.4 | HTTPS/mTLS direct-peer connector using exact hostname/SNI and certificate pinning, with no plaintext fallback | AO.2 |
| AO.5 | Lifecycle, doctor, endpoint-owner, boundary, and negative architecture guards for the optional TLS interface | AO.3, AO.4 |
| AO.6 | Physical proof on a selected peer pair and retained report/index evidence | AO.5 |

### AO.1 — Contract, policy, and test seams

- Inventory the actual active client, listener, bootstrap, `PeerConfigStore`,
  doctor, and endpoint-publication seams on the accepted base.
- Define the closed transport-mode configuration and a typed TLS error set:
  missing/invalid key material, disabled interface, unknown peer, hostname
  mismatch, certificate pin mismatch, client-certificate rejection, handshake
  failure, and deadline/cancellation. Each error carries a stable recovery
  action without leaking certificate/key material.
- Specify the configuration migration and operator commands. Existing peer
  rows remain unchanged until explicitly opted into mTLS.
- Add contract tests that prove a TLS transport cannot be selected without all
  mandatory configuration, and that an opted-in peer has no plaintext option.

**Accept when:** reviewers can trace every TLS decision from `HostName` and
trust configuration through a typed connector/listener contract, and no new
public extension trait or open implementation surface is introduced.

### AO.2 — Material loading and pre-bind validation

- Implement owner-only loading of certificate/key material from the opaque
  `PrivateKeyRef`; durable stores and doctor keep only public data.
- Validate parseability, certificate/key pairing, local identity, configured
  bind address, enabled trusted peers, and fingerprint format before binding.
- Reject a contradictory or incomplete configuration with a typed error and no
  listener, endpoint record, or background task.

**Accept when:** malformed keys, mismatched keys, disabled/unknown peers, and
invalid trust data fail before bind/publication; secret bytes are absent from
errors, logs, and serialized status.

### AO.3 — Authenticated listener ingress

- Add a Tokio TLS acceptor around the existing HTTP server service, not a
  second router or peer-specific decoder.
- Require and validate a client certificate. Map the verified fingerprint to
  the enabled exact trusted hostname before constructing authenticated ingress
  provenance.
- Enforce existing HTTP body/concurrency/deadline controls after the handshake
  and before application dispatch.

**Accept when:** accepted mTLS traffic reaches the existing handler exactly
once; wrong/missing/disabled/mismatched client certificates cannot reach
router/storage/nudge; TLS and local listener shutdown share lifecycle rules.

### AO.4 — Authenticated direct-peer client

- Extend the shared client connector selection with the `MutualTls` connector.
- Use the configured registered hostname as the TLS server name/SNI and
  validate the configured pinned certificate fingerprint after standard TLS
  validation.
- Preserve one absolute request deadline across DNS, TCP connect, TLS
  handshake, request write, and response read.

**Accept when:** an mTLS-selected peer issues the unchanged canonical HTTP
request, validates authority before write, emits a typed failure on every
negative connection case, and never dials plaintext as fallback.

### AO.5 — Ownership and regression guards

- Integrate TLS interface ownership into daemon lifecycle, stop/drain, doctor,
  and endpoint-record publication.
- Add architecture tests proving one active router, no runtime dependency on
  `atm-peer-tls-interop`, no client selection outside the shared connector
  owner, and no legacy daemon use.
- Add cross-platform tests for disabled mode, opt-in mode, invalid
  configuration, concurrent start/stop, and no duplicate listener/publisher.

**Accept when:** static guards and lifecycle tests make it impossible to
reintroduce a second application path, fixture dependency, fallback, or
legacy-daemon execution through TLS work.

### AO.6 — Physical proof and release record

- Select two hosts with verified reachability and record their exact candidate
  SHA, version, operating system, hostname, certificate fingerprints (public
  identifiers only), and report paths.
- Prove bidirectional canonical send/read/requires-ack/reply over mTLS plus
  negative wrong-cert, wrong-hostname, disabled-peer, and plaintext-on-TLS-port
  cases.
- Run the existing local UDS/loopback/plain-direct regression matrix and
  `just lint`/`just test`; render and index reports under `site/reports/`.

**Accept when:** the retained report proves same-handler delivery over mTLS,
every negative case fails before application dispatch, and the candidate has no
TLS downgrade path.

## Verification matrix

| Layer | Required proof |
| --- | --- |
| Unit | transport-mode validation; key/certificate pairing; hostname/pin comparison; error codes and recovery text; no-secret serialization |
| Integration | accepted mTLS write reaches normal router/storage/hook; rejected TLS reaches none; precise deadline/cancellation behavior |
| Lifecycle | one listener/publisher owner; failed bind/validation leaves `NotReady`; drain stops accept before record cleanup |
| Architecture | no legacy daemon execution; no fixture-crate production edge; one client selector; no TLS-to-plaintext fallback |
| Physical | selected two-host bidirectional send/read/ack/reply, negative certificate/hostname/plaintext cases, reports indexed |
| Regression | `just lint`, `just test`, local UDS/loopback/plain-direct smoke, and the current cross-host smoke ladder |

## Rust boundary review requirements

- **RBP-001:** TLS failures require structured error code, cause, and safe
  recovery guidance.
- **RBP-002:** listener/session lifecycle must make pre-authenticated traffic
  unable to construct authenticated provenance.
- **RBP-003 / RBP-008:** do not open a public connector/listener extension
  trait merely for TLS; use the existing sealed runtime boundary unless an ADR
  explicitly changes it.
- **RBP-004:** host, fingerprint, key reference, and certificate material keep
  their validated domain types; no new raw `String` authority plumbing.
- **RBP-006:** any shared TLS config/cache mutation needs explicit ownership
  rationale and bounded lifecycle cleanup.

## Non-goals

- Solving a corporate firewall/NAT reachability problem. AO authenticates a
  connection after one can be made; it does not make inbound reachability
  exist.
- WebSockets, SSE, long-polling, reverse tunnels, brokers, or a relay.
- Automatic certificate issuance, key escrow, certificate rotation service,
  discovery through IP/reverse DNS, or public internet exposure.
- Delivery queues, replay/retry, mailbox synchronization changes, or a new
  message/nudge format.

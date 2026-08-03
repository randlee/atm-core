---
title: Phase AK Plan — direct peer HTTP delivery
status: proposed
branch: plan/mvp-simplification
worktree: ../atm-core-worktrees/plan/mvp-simplification
target: integrate/phase-ak
---

# Phase AK — direct peer HTTP delivery

## Goal

Replace daemon-owned peer workers, custom DNS, and custom mTLS transport with
one direct HTTP request function. A host-qualified write uses the same request
shape as local HTTP. The receiving daemon persists it and emits the ordinary
nudge.

Phase AK starts only after Phase AI merges to `develop`, on
`integrate/phase-ak`. It does not alter the already-planned Phase AJ
runtime-observation work. AK.2's temporary no-delivery state must never merge
to `develop`; AK.4 restores and proves direct delivery on this phase branch.

## MVP contract

| Concern | Decision |
| --- | --- |
| Peer configuration | Canonical full hostname plus explicit aliases and port. Configuration changes populate a small alias index. SQLite stores the full hostname, never a resolved IP. |
| Wire | HTTP/1.1 `POST /v1/atm/messages`, JSON `WriteRequest`, JSON `SendResponseEnvelope`. `send_peer_http` accepts an array but issues this one existing request shape per immutable write; AK adds no batch route or second receiver handler. |
| Security | Explicit trusted-LAN MVP: no mTLS, certificate pinning, or claimed-source authentication. The sender-host header is display provenance only. Internet exposure is prohibited. |
| Receiver | The existing HTTP decoder, canonical write handler, SQLite transaction, ACK semantics, and post-write nudge remain the only ingress path. |
| Sender | CLI and graft use their existing local daemon HTTP call. After canonical SQLite persistence, the daemon makes one private `send_peer_http(endpoint, &[write])` call with the in-memory write and returns the ordinary response. |
| AK.4 baseline | No automatic retry. A failed direct send returns an ordinary delivery error and leaves the admitted SQLite record undelivered. |
| AK.5 resend cache | Adds optional per-endpoint `Connected`/`Disconnected`/`Queued` state, one aggregate, and one timer. `peer_resend_cache = true` is the default; `false` preserves AK.4's no-retry behavior. |
| Failure | Ordinary typed send failure; no receipt synthesis, remote mailbox mutation, or local nudge fallback. |

The plan intentionally does not invent a second cross-host protocol, a curl
subprocess, client-side SQLite ownership, a client-only payload, or a
local/cross-host inbound branch. `curl` is the executable proof for this
ordinary HTTP request; the production call uses the same HTTP request/response
contract in-process.

## Required removal

Delete, not adapt:

- `crates/atm-daemon/src/peer_drain_coordinator.rs` and its composition,
  shutdown, tests, `PeerDeliveryCoordinator`, per-message threads, job state,
  and peer delivery post-commit queue key.
- `crates/atm-daemon/src/peer_resolution.rs` and literal-IP authority
  discovery in `runtime_health/peer_authority.rs`.
- Active `HttpsTransport`, `PinnedClientVerifier`, `TlsIdentity`, custom
  rustls client/server handshake code, and certificate/fingerprint peer
  transport configuration once the plain HTTP listener is live. AK.6 preserves
  verified TLS provisioning/configuration and curl-interoperable receiver
  support only in an isolated, unused TLS crate. That crate is not a native
  peer transport: no native ATM TLS sender is known to work. It does not
  preserve the failing native outbound client in active code.
- Worker-only peer recovery/replay observability, policies, and documentation.

The existing simple-send path is intentionally reduced in explicit steps:

| Current step | AK owner | Result |
| --- | --- | --- |
| Save the immutable write locally. | Retain | Canonical SQLite admission remains first. The origin record retains its immutable write plus destination host; this is durable message data, not worker state. |
| Queue only `{ hostname, message_id }`. | AK.2 | Delete. |
| Start a coordinator thread. | AK.2 | Delete. |
| Start a per-message thread. | AK.2 | Delete. |
| Re-scan SQLite for the write just saved. | AK.2 | Delete. AK.4 uses the in-memory `WriteRequest` for an immediate send. |
| Read every trusted-peer row. | AK.3/AK.6 | AK.3 creates the O(1) alias-to-full-host index; AK.6 deletes the old broad scan. |
| Resolve every peer hostname for literal-IP alias discovery. | AK.3/AK.6 | AK.3 permits only explicit configured IP aliases; AK.6 deletes inferred discovery. |
| Start a DNS thread for the selected peer. | AK.6 | Delete. Resolve the persisted full hostname only when connecting; never in an ATM DNS thread. |
| Open custom TCP/rustls HTTP. | AK.4/AK.6 | AK.4 creates its replacement; AK.6 deletes the unused legacy stack. |

Retain unchanged unless a direct compile consumer proves otherwise:

- `WriteRequest`, origin ULID idempotency, `ResponseEnvelope`, HTTP framing,
  body limits, canonical persistence, receiver ACK transition, and ordinary
  post-write nudge.
- Host-qualified address parsing and the exact host/port peer alias row.

## Non-negotiable checks

1. A host-qualified message delivered by `curl` and by the production direct
   call reaches the same receiver handler, persists the same ULID, renders the
   claimed source host, and produces the same nudge.
2. A source gate rejects `peer_drain_coordinator`, `PeerDeliveryCoordinator`,
   `PeerWork`, `PeerJob`, `PinnedClientVerifier`, `TlsIdentity`, `rustls`,
   `peer_resolution`, custom peer DNS threads, and per-message peer threads.
3. No production route makes a local-host/same-IP exception after ingress.
4. AK.5's only batch path calls the same `send_peer_http` function as AK.4's
   immediate path. It adds no second serializer, parser, delivery protocol, or
   nudge route.
5. With `peer_resend_cache = false`, a failed direct send creates no automatic
   resend aggregate or timer and returns a delivery error.
6. `just lint` and `just test` pass. Cross-host evidence includes M4→M5 and
   M5→M4 curl and production-call send, receive, and nudge.

## Sprint order

| Sprint | Closure | Dependencies | Recommended agent |
| --- | --- | --- | --- |
| AK.1 | Recover cross-host ACK/provenance from `fix/crosshost-ack-provenance`: audit, retain the useful fixes, and prove remote curl message/ACK/nudge behavior. | Must follow Phase AI merge to `develop`; lands only on `integrate/phase-ak`. | arch-ctm |
| AK.2 | Delete the daemon peer worker and all worker-only state. | Must follow AK.1 development push and merge-forward. AK.1 PR must merge before AK.2 PR completion. | arch-ctm |
| AK.3 | Normalize configured aliases to full hostnames before persistence, with no delivery behavior change. | Must follow AK.2 development push and merge-forward. AK.2 PR must merge before AK.3 PR completion. | arch-ctm |
| AK.4 | Prove direct full-host HTTP delivery with no retry. | Must follow AK.3 development push and merge-forward. AK.3 PR must merge before AK.4 PR completion. | arch-ctm |
| AK.5 | Add optional default-on resend caching through AK.4's proven HTTP function. | Must follow AK.4 development push and merge-forward. AK.4 PR must merge before AK.5 PR completion. | arch-ctm |
| AK.6 | Delete now-dead peer scans, literal-IP discovery, DNS threads, and active custom TLS; isolate provisioning/receiver curl interop. | Must follow AK.5 development push and merge-forward. AK.5 PR must merge before AK.6 PR completion. | arch-ctm |

Each dependent sprint begins immediately after its predecessor development is
pushed and merge-forwarded; it does **not** wait for predecessor QA approval.
Every dependent development or fix round first merges its predecessor. A
dependent PR cannot complete before its predecessor PR merges.

## Governing changes

AK.1 records and implements the surviving cross-host provenance fixes. AK.2 updates the
project plan and retires worker-specific AI.28/AI.31/AI.32 claims. AK.6
replaces ADR-034/ADR-035 transport text and the corresponding
requirements/boundary rules: peer host alias configuration remains; custom
TLS/pinning, literal-IP authority discovery, and daemon worker delivery do not.

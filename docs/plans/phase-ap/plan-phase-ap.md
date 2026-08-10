---
title: Phase AP Outline — Outbound-Only Corporate Network Peer Connectivity
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AP — Outbound-Only Corporate Network Peer Connectivity

## Goal

Support a daemon that cannot accept unsolicited inbound peer TCP connections
because of corporate firewall, NAT, VPN, DNS, or proxy policy. The proposed
product direction is an authenticated outbound Server-Sent Events (SSE) session
from that restricted daemon to a reachable peer, plus ordinary authenticated
HTTP POST for its response leg.

This is a separate transport feature. It is not a workaround to fabricate the
missing Windows physical-proof row and it does not change Phase AL's direct
peer contract.

## Mandatory first gate: prove the real network path

**AP.1 is the first implementation and acceptance gate. No product transport
code begins until it passes.** It must run on the actual current CWin, M4, and
M5 machines—not a container, localhost simulation, SSH tunnel, port forward,
or substitute relay.

The current evidence says CWin lacks DNS/routing to the M4/M5 direct-peer
network. AP.1 may therefore fail or block. That is a useful result: record the
precise policy/route/proxy constraint and stop before AP.2. Do not "fix" a
failed AP.1 by adding a second daemon, raw-IP authority, an alternate runner,
or an unapproved external service.

### AP.1 proof protocol

1. Freeze the candidate SHAs/versions for CWin, M4, and M5 and record each
   host's network interface, DNS result, default route, configured proxy, and
   relevant outbound policy—without storing credentials or private tokens in
   the report.
2. From CWin, test DNS and outbound TCP/TLS reachability to one nominated M4
   service hostname and port. From M4 and M5, record whether CWin accepts
   unsolicited inbound traffic; the expected restrictive result is permitted.
3. Run a non-production, mTLS-authenticated HTTP/1.1 SSE demonstrator on the
   real nominated M4 endpoint. CWin must initiate the TLS connection, retain
   the stream across the declared observation interval, receive a correlated
   test event, and submit a normal authenticated POST response on its own
   outbound connection.
4. Intentionally disconnect and reconnect CWin once. Record proxy behavior,
   certificate/SNI behavior, reconnect outcome, and whether the same route is
   available after reconnect. This proves viability only; it does not establish
   future replay/delivery guarantees.
5. Place a machine-readable report and an XHTML panel under `site/reports/`,
   regenerate the reports index, and record the exact command, candidate, host
   labels, and pass/fail evidence.

**AP.1 passes only when** CWin establishes the outbound mTLS SSE stream to the
real M4 endpoint, receives the test event, sends the correlated POST response,
and reconnects through the same real network policy. SSH reverse tunnels,
localhost relays, arbitrary raw-IP substitutions, and a different machine do
not count. A documented blocked result is the correct completion result when
the network does not permit the path.

## Why SSE + POST is preferred

SSE is ordinary HTTP/1.1 response streaming, which is generally less fragile
than a WebSocket upgrade under restrictive corporate proxies. The restricted
host remains the connection initiator. Its response leg is an ordinary POST,
so no separate bidirectional socket protocol is needed.

Long-poll has the same authentication and correlation requirements while
repeatedly reopening requests. WebSocket has the cleanest duplex shape but
adds a new framing protocol and proxy-upgrade risk. SSH reverse tunnels are a
useful operator diagnostic if separately approved, but not a product transport
or proof of native corporate-network support. A durable third-party relay is a
different product with storage/availability/privacy obligations and is out of
scope for AP.

## Proposed transport contract after AP.1 passes

The restricted daemon owns one authenticated outbound session to a reachable
peer. The reachable peer keeps only an in-memory registry of live sessions.
When a local sender selects the restricted host, the shared client selector
chooses the live outbound-session transport:

```text
sender CLI/graft
  -> canonical WriteRequest
  -> shared transport selector
  -> reachable-peer live SSE session
  -> restricted daemon's existing HTTP write handler
  -> existing storage + post-receive nudge
  -> correlated ordinary HTTPS POST response
  -> original caller result
```

The SSE event carries a transport envelope containing the unchanged canonical
write request plus a correlation identifier. The restricted daemon must feed
that request through its existing authenticated HTTP write handler or the same
typed handler entry point—not synthesize a nudge, Telegram event, mailbox row,
or alternate message stream. The post-receive nudge occurs only after normal
persistence, exactly as for local/direct-peer delivery.

### Direct-delivery semantics

AP is deliberately **online-only** at first:

- A live session is required before the reachable peer accepts a restricted
  destination request for transport.
- The sender gets success only after the restricted daemon returns the normal
  canonical HTTP result through the correlated response POST.
- Missing, expired, overloaded, disconnected, or unauthorized sessions return
  a typed direct-delivery failure to the caller.
- The registry may have bounded in-memory flow control for a live connection,
  but it may not persist messages, schedule retries, replay events, retain an
  outbox, or turn into a store-and-forward relay.

This preserves the important distinction between a temporarily live transport
session and durable delivery state. If a future requirement needs offline
delivery, it must be proposed as a separate durable-relay phase with explicit
retention, authorization, privacy, and recovery semantics.

## Entry gate

AP product implementation requires all of the following:

1. AP.1 real-host feasibility proof is retained and indexed as PASS.
2. Phase AO mTLS is accepted on the active runtime, or AP has an independently
   reviewed equivalent mTLS owner. The session identity cannot rely on a
   caller-supplied hostname/header, IP address, or secret bearer token.
3. The active Tokio/Axum line is the only daemon runtime. Frozen legacy daemon
   code is not eligible for modification, fallback, or testing.
4. The chosen reachable M4 endpoint, certificate authority, and operational
   owner have explicit approval. No public exposure is inferred from AP.1.

## Planned sprint sequence

| Sprint | Closure | Depends on |
| --- | --- | --- |
| AP.1 | Real CWin↔M4/M5 outbound SSE + POST feasibility evidence, or a documented infrastructure block | none; executes first |
| AP.2 | Reviewed active-session model, correlation/error contract, bounded flow control, and architecture guards | AP.1 PASS, AO security gate |
| AP.3 | mTLS SSE session listener/client with authenticated session registration and lifecycle ownership | AP.2 |
| AP.4 | Canonical write/response bridging through the existing handler, no synthetic nudge or alternate mailbox path | AP.3 |
| AP.5 | Doctor, operator controls, reconnect/overload behavior, reports, and real CWin proof | AP.4 |

### AP.2 — Session contract and bounded state

- Define validated `OutboundSessionId` and `DeliveryCorrelationId` types.
- Model the session lifecycle as `Connecting -> Authenticated -> Live ->
  Closed`; no delivery operation may use a non-live or unauthenticated session.
- Define stable errors with recovery: session unavailable, authentication
  rejected, bounded flow-control refusal, correlation expired, peer result
  rejected, and deadline/cancellation.
- Specify per-session frame/body limits, heartbeat policy, a bounded number of
  in-flight correlations, and immediate failure on capacity exhaustion.

**Accept when:** the model has no persistence, retry worker, implicit host
claim, or unbounded queue; every result maps to the existing canonical caller
outcome shape with safe recovery text.

### AP.3 — Authenticated SSE session

- The restricted daemon initiates mTLS to the nominated reachable peer and
  opens an HTTP/1.1 SSE stream.
- The reachable peer derives the exact remote hostname from the authenticated
  certificate/trust record, registers one live session under that identity,
  and removes it on disconnect/ownership change.
- Heartbeats/reconnect are lifecycle signals, not message retries. A
  disconnected session fails new sends until the restricted daemon reconnects.

**Accept when:** forged headers, duplicate sessions, disabled peers, wrong
certificates, stale sessions, and stream overflow cannot deliver a request;
only the current authenticated session owns its registry record.

### AP.4 — Canonical request/response bridge

- Send a canonical `WriteRequest` envelope over the live SSE session with a
  bounded correlation identifier.
- At the restricted daemon, call the same normal write handler used for local
  and direct-peer HTTP ingress. Persist first, then emit the normal local
  post-receive nudge.
- Return the canonical result over ordinary authenticated POST and complete
  the original pending request at the reachable peer.

**Accept when:** source/provenance, message id, body, requires-ack, and result
are preserved end-to-end; no synthetic nudge, direct SQLite access, second
router, transport-owned mailbox mutation, or automatic retry exists.

### AP.5 — Operations and physical proof

- Extend doctor with safe session state: configured mode, authenticated/live
  state, peer hostname, generation, last state transition, and bounded
  in-flight count. It must not reveal key material or message bodies.
- Provide explicit enable/disable and status commands. Disable drains current
  work to the bounded deadline and removes the live registry entry before
  completion.
- Run a real CWin↔M4 end-to-end proof: CWin outbound connection, M4-originated
  canonical send, CWin persistence/nudge, correlated result, requires-ack/reply
  where applicable, disconnect/reconnect, disabled peer, wrong certificate,
  saturated capacity, and no-live-session failure.

**Accept when:** all physical and negative results are reported/indexed at one
candidate SHA; normal local/direct peer regression remains green; the evidence
does not rely on a tunnel, synthetic loopback, or external relay.

## Security and Rust boundary requirements

- **RBP-001:** Every connection/session/correlation failure has a typed code,
  context, and recovery instruction; no TLS/private data leaks.
- **RBP-002:** authenticated/live session state must be represented so an
  unauthenticated or closed session cannot enqueue a delivery.
- **RBP-003 / RBP-008:** session transport remains an internal sealed runtime
  concern, not a public plugin trait.
- **RBP-004:** host, session id, correlation id, and limits are validated
  domain types, never interchangeable raw strings/integers.
- **RBP-006:** the in-memory registry has one runtime owner, bounded mutation,
  and explicit disconnect cleanup; it is not a hidden durable queue.
- `atm-http-runtime` owns HTTP/SSE and connection lifecycle; transport code
  never reaches SQLite or produces nudge payloads directly.

## Explicit non-goals

- Claiming a blocked AP.1 as a product pass or modifying CWin network policy
  without its operator's authorization.
- SSH reverse tunnels, VPN alteration, raw-IP identity, public exposure, or a
  third-party relay as the product implementation.
- WebSocket or long-poll production support in AP.
- A durable outbox, store-and-forward relay, replay/retry scheduler, offline
  mailbox replication, or delivery receipt subsystem.
- Changing user-facing message text or treating a nudge as message transport.

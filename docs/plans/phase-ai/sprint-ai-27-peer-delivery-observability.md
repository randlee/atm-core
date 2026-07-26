---
title: AI.27 truthful peer delivery outcomes
status: complete
branch: feature/pAI-s27-peer-delivery-observability
target: integrate/phase-AI
depends_on: AI.21-pre, AI.23, AI.26
---

# AI.27 — truthful peer delivery outcomes

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-27`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

CLI errors, retained daemon events, and `atm doctor --json` distinguish
persisted, confirmed, and unconfirmed peer writes. Each configured peer has a
derived link-quality projection without a second delivery-state store.

## Deliverables

1. Consume ADR-041's `RemoteDeliveryUnconfirmed` mapping, implemented by
   AI.26, for local
   response-read timeout after dispatch; reserve `DAEMON_UNAVAILABLE` for
   actual local daemon unavailability.
2. Emit the retained event schema below for connection-handler, route, and
   response-write failures with request/message correlation.

   ```rust
   #[derive(Clone, Copy, serde::Serialize)]
   #[serde(rename_all = "snake_case")]
   pub enum PeerDeliveryEventKind {
       WritePersisted,
       PeerDeliveryConfirmed,
       PeerDeliveryUnconfirmed,
   }

   pub struct PeerDeliveryEvent {
       pub kind: PeerDeliveryEventKind,
       pub request_id: RequestId,
       pub message_id: Option<MessageId>,
       pub peer: HostName,
       pub error_code: Option<AtmErrorCode>,
   }
   ```

   `RemoteDeliveryUnconfirmed` from AI.26 is the synchronous API error when
   peer acceptance is unknown. `peer_delivery_unconfirmed` is this sprint's
   retained terminal event for that same result. AI.28 introduces its own four
   `peer_recovery_*` variants and recovery-only event fields when bounded
   recovery exists; AI.27 exposes no unused recovery event surface.
   The event deliberately contains the registered hostname only: never a
   certificate pin, private-key reference, body, or resolved IP.
3. Add the following bounded, in-memory projection to the daemon runtime and
   `DoctorReport`; `atm doctor --json` is the initial operator display surface:

   ```rust
   pub enum PeerLinkQuality { Healthy, Degraded, Unreachable, Misconfigured }
   pub enum PeerDrainState { Idle, Connecting, Draining }

   pub struct PeerLinkStatus {
       pub peer: HostName,
       pub quality: PeerLinkQuality,
       pub last_success_at: Option<IsoTimestamp>,
       pub last_failure_at: Option<IsoTimestamp>,
       pub last_error_code: Option<AtmErrorCode>,
       pub next_attempt_at: Option<IsoTimestamp>,
       pub drain: PeerDrainState,
       pub candidate_count: Option<u32>,
   }
   ```

   `DaemonRequestDispatcher::record_peer_delivery_event` in
   `crates/atm-daemon/src/runtime_health.rs` is the sole projection writer:
   it receives the retained event and updates `PeerLinkStatus`/
   `PeerDrainState`. AI.28 emits recovery facts to that function; it never
   writes the projection directly.

   `PeerLinkStatus` is a lossy observability snapshot. It stores no message
   IDs, payloads, cursors, delivery receipts, or authority material; restart
   resets it to `Misconfigured` or `Degraded` until an ordinary attempt
   establishes a newer fact.
4. Replace pre-peer `outcome sent` with `write_persisted`; emit
   `peer_delivery_confirmed` only after peer HTTP acceptance, otherwise
   `peer_delivery_unconfirmed`.
5. Update smoke evidence parsers and user-facing recovery text to reject local
   persistence as receiver proof.

## Implementation map

- `crates/atm-core/src/error_codes.rs` and `error.rs`: own the one typed
  `RemoteDeliveryUnconfirmed` catalog entry and safe recovery text.
- `crates/atm-daemon/src/runtime_health.rs`:
  `DaemonRequestDispatcher::record_peer_delivery_event` is the sole event-to-
  projection writer. It emits the specified events from the canonical
  route/post-write outcome and maintains the bounded projection; do not add
  transport-local status state.
- `crates/atm-core/src/doctor/report.rs` and `doctor/mod.rs`: add the
  secret-free `PeerLinkStatus` projection to `DoctorReport`.
- `scripts/smoke/analyze_logs.py`: consume event names only; it must not infer
  receiver acceptance from persistence or TCP connect logs.

## Acceptance criteria

- A locally persisted remote write cannot be reported as sent before the peer
  response arrives.
- Every terminal error is observable and maps to one ADR-032 error value.
- A local read timeout has a truthful typed outcome and recovery guidance.
- `atm doctor --json` reports one secret-free, bounded status row per configured
  peer; a failed attempt changes it to `Degraded` or `Unreachable`, and a peer
  HTTP acceptance changes it to `Healthy`.
- No new receipt, delivery state, or ack-specific route is introduced.

## Required validation

Tests for response-write failure, route failure, peer success, peer uncertainty,
and error-code mapping; doctor status transition/secret-exclusion assertion;
event-schema assertion; `just lint`; `just test`.

## Non-closure

Physical peer proof belongs only to AI.29. Delivery-event emission must occur
within the same shared dispatch/write path AI.23 establishes
(`Arc<dyn RequestDispatcher>` via `composition.rs`'s `request_dispatcher()`
accessor) — this sprint does not introduce a second event-emission or
write/nudge implementation for peer-originated messages. AI.28 owns all
reconnect timing and single-flight coordination; this sprint only projects its
observable facts.

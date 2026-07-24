---
title: AI.24 truthful peer delivery outcomes
status: proposed
branch: feature/pAI-s24-peer-delivery-observability
target: integrate/phase-AI
depends_on: AI.23
---

# AI.24 — truthful peer delivery outcomes

## Closure

CLI errors and retained daemon events distinguish persisted, confirmed, and
unconfirmed peer writes; no sender-side event overstates remote receipt.

## Deliverables

1. Add the ADR-041 typed error mapping for local response-read timeout after
   dispatch; reserve `DAEMON_UNAVAILABLE` for actual local daemon unavailability.
2. Emit structured terminal events for connection-handler, route, and
   response-write failures with request/message correlation.
3. Replace pre-peer `outcome sent` with `write_persisted`; emit
   `peer_delivery_confirmed` only after peer HTTP acceptance, otherwise
   `peer_delivery_unconfirmed`.
4. Update smoke evidence parsers and user-facing recovery text to reject local
   persistence as receiver proof.

## Acceptance criteria

- A locally persisted remote write cannot be reported as sent before the peer
  response arrives.
- Every terminal error is observable and maps to one ADR-032 error value.
- A local read timeout has a truthful typed outcome and recovery guidance.
- No new receipt, delivery state, or ack-specific route is introduced.

## Required validation

Tests for response-write failure, route failure, peer success, peer uncertainty,
and error-code mapping; event-schema assertion; `just lint`; `just test`.

## Non-closure

Physical peer proof belongs only to AI.25.

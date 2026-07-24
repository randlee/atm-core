---
title: AI.22 DNS-backed peer authority
status: proposed
branch: feature/pAI-s22-peer-authority-resolution
target: integrate/phase-AI
depends_on: AI.11–AI.16
---

# AI.22 — DNS-backed peer authority

## Closure

One registered hostname/HTTPS-port/certificate pin authorizes its current
DNS-resolved IP addresses without durable IP aliases, and trust mutations take
effect in the one live daemon without a restart.

## Deliverables

1. Define `PeerAuthority` as registered hostname, HTTPS port, and pinned
   fingerprint behind the existing storage trait; delete any peer-IP-as-
   authority lookup.
2. Add bounded fresh DNS resolution: hostname targets exact-match a registered
   authority; literal IP targets match exactly one authority's current A/AAAA
   result. Zero or multiple matches fail closed with typed errors.
3. Preserve the chosen registered hostname and port for TLS authority/pin verification;
   reject reverse-DNS inference and never persist resolver output. Document
   that the peer operator maintains the hostname's forward DNS/DDNS record as
   its VPN/Wi-Fi address changes.
4. Add daemon-owned atomic refresh of live trust verification after CLI trust
   add/replace/revoke. No second daemon, listener fallback, or direct SQLite
   access outside storage traits.

## Acceptance criteria

- Registering `fastpc4.rz.local` permits both its hostname and a currently
  resolved literal IP with the same certificate pin.
- A changed DNS answer is honored on the next resolution; stale IP no longer
  authorizes.
- A forward DNS change models a VPN address change without reverse DNS,
  SQLite mutation, daemon replacement, or a second trust record.
- An IP matching zero or multiple registered names fails before TLS/route.
- An IP-only record does not authorize a hostname, and no resolver result is
  written to SQLite.
- A live trust mutation changes the current daemon verifier without process
  replacement; tests prove one daemon remains.
- Two account daemons whose endpoint names resolve to one IP can be trusted
  independently on distinct configured ports; an occupied port fails closed
  rather than falling back to another listener.

## Required validation

Unit tests for exact/zero/ambiguous resolution and pin selection; integration
test for live trust refresh; structural test that transport adapters do not
import SQLite; `just lint`; `just test`.

## Non-closure

This sprint does not change write deadlines, delivery outcomes, or physical
peer evidence.

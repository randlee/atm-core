# Phase AK — Cross-Host Mandate Compliance: Findings and Fix Scope

Status: DRAFT for operator review
Scope: `integrate/phase-ak` as merged, plus in-flight AK.11 work on
`feature/pak-s11-m5-crosshost-proof` (uncommitted at time of review)
Author: team-lead audit, 2026-08-05

---

## 1. The mandate (restated)

Phase AK was chartered as a deletion phase. Cross-host send:

1. **SHALL be no different than a local-host send.**
2. **SHALL use the same remote endpoint on the remote server that local
   cli/graft use.**
3. **SHALL use the same send logic used for cli/graft → daemon**
   (shared send-message code).

One optional extension was authorized: if a send fails, keep a cursor of
what was successfully sent, and a **very simple, single, timer-driven state
machine** may resend `messages[]` to the same endpoint. This extension is
**optional and disableable** — it must never change the direct-send path.

## 2. What phase AK actually delivered (as merged)

The old stack was genuinely deleted (≈ −3,550 lines: `https_transport`,
`peer_drain_coordinator`, `peer_delivery_observability`, `peer_authority`,
`peer_resolution`, reconciliation tests). However, a peer-specific stack of
comparable size was rebuilt in its place, and the mandate was violated on
all three SHALLs:

| # | Mandate | As merged on `integrate/phase-ak` | Verdict |
|---|---------|-----------------------------------|---------|
| 1 | No different than local send | Every peer send — including a single direct send — uses the peer-only `PeerMessageArray { messages: [...] }` body, a peer-only `X-ATM-Peer-Source-Host` header, and a peer-only ingress class. `send_peer_http_batch` is documented as "the sole production peer sender for both direct singleton delivery and bounded recovery pages." | **Violated** |
| 2 | Same remote endpoint | Route path (`POST /v1/atm/messages`) and internal `ApiRouter` are shared, but ingress is a physically separate listener set (`PeerHttpListenerSet`) with its own decoder (`decode_peer_write_request`, writes-only), connection registry, capacity cap, and budget. | **Half-met** |
| 3 | Same shared send code | cli/graft send via `atm-daemon-client`; the daemon's peer sender is a second hand-rolled HTTP client sharing only the lowest-level framing helpers. | **Violated** |
| — | Optional extension | AK.5 resend cache **defaults ON** (mandate: optionally enabled) and keeps a per-endpoint state map. The `messages[]` format authorized for recovery became the format for **all** peer sends. | **Drifted** |

Root cause of the drift: ADR-046 and ADR-047 were written and accepted
mid-phase, codifying the divergences as decisions. QA then validated code
against the ADRs instead of against the mandate, so every gate passed.

**Explicit exception (operator-confirmed):** the TLS material in
`atm-peer-tls-interop` and `atm-storage/src/tls.rs` is **working code
intentionally preserved in a quarantined crate for future reference**. It is
production-isolated (a boundary test forbids the daemon edge). It is NOT a
finding and MUST NOT be deleted.

## 3. AK.11 in-flight review (`feature/pak-s11-m5-crosshost-proof`)

Verdict: **progressing in the right direction — continue, do not abandon.**
The uncommitted work is deletion-heavy (−1,496 / +296) and lands directly on
the mandate:

- New `peer_delivery_client.rs::send_configured_peer_write` sends **one
  ordinary singleton write** (`RequestEnvelope::Write`) through the
  **canonical shared HTTP writer/reader** (`write_http_request_with_headers`
  / `read_http_response`) — the same wire format and serialization code the
  local CLI and graft use. Fixes mandate #1 and substantially #3.
- `PostWriteRouter` now calls the singleton sender directly: "A
  host-qualified write is one immediate request" — no scheduler branch.
- AK.5's `PeerResendScheduler` is retired to a compile-failing tombstone;
  `peer_resend_cache` always reports disabled and `set true` is rejected.
  This is **stronger than** the mandate's default-off requirement and
  consistent with the standing operator preference to remove a risky resend
  rather than ship a patched one.
- Requirements/ADR text is being reconciled in the same change
  ("AK.11 retires AK.5's resend cache… No active automatic resend
  replacement exists.").

Residual gaps AK.11 does not yet close: ingress is still peer-forked
(`decode_peer_write_request`, separate listener semantics), and the
`PeerMessageArray` grammar plus its support code remain in the tree with no
production sender.

## 4. Fix scope

### F1 — Direct send uses the canonical singleton write  ✅ in AK.11
Accept when: the only production peer sender emits `RequestEnvelope::Write`
via the shared writer; `send_peer_http_batch` has no production caller.
Owner: AK.11 (already implemented in the worktree; needs commit + QA).

### F2 — Shared send logic  ✅ in AK.11 (accept at the atm-core layer)
The daemon cannot depend on `atm-daemon-client` without inverting crate
boundaries. Sharing the full request/response wire path in `atm-core::api`
(writer, reader, response matching) satisfies the mandate's intent; the
peer client may own only connection setup (resolve/connect/timeouts) and
the provenance header. Accept when: no peer-only request serializer exists
on the direct path. Owner: AK.11.

### F3 — Ingress unification (remaining work)
Collapse `decode_peer_write_request` into the ordinary `decode_request`
plus a thin provenance layer. Permitted peer-listener differences are
limited to: bind address, `X-ATM-Peer-Source-Host` extraction, and
`AuthenticatedIngress::Peer` tagging. The writes-only restriction and any
peer-only body grammar must not live in a parallel decoder.
Accept when: a cross-host frame and a local frame are decoded by the same
function, and curl can POST the identical singleton body to either
listener with the same result. Owner: follow-up sprint (AK.12 candidate).

### F4 — `PeerMessageArray` disposition (remaining work)
With resend retired there is no production sender of `messages[]`.
Frame as **forgot-to-delete**: remove `peer_array.rs`, the `messages[]`
branch of the peer decoder, `ApiRequest::PeerMessages` routing, the
`peer_message_array` tests, and the resend accessors left in
`peer_config_store` (beyond the compatibility `set false` surface AK.11
keeps). If recovery is ever revived, it re-enters under the original
allowance: default-OFF, one timer-driven state machine, `messages[]` to
the same endpoint — as a new approved sprint, not as retained scaffolding.
Owner: follow-up sprint (may combine with F3).

### F5 — ADR reconciliation (remaining work)
ADR-047's "the origin emits one ordered `PeerMessageArray`… a direct send
is the one-element case" and ADR-046's default-on cache language are now
wrong twice over (vs the mandate and vs AK.11). Amend or supersede both so
the accepted ADR set matches the mandate: direct send = canonical singleton
write; no active resend. QA gates must cite the mandate section of this
document, not superseded ADR text. Owner: AK.11 doc pass or follow-up.

## 5. Non-goals / guardrails

- **Do not delete** `atm-peer-tls-interop` or `atm-storage/src/tls.rs`
  (operator-directed quarantine of working TLS code; boundary test keeps it
  out of production).
- No re-hydration of any deleted machinery: no outbound workers, resolver
  threads, connection pools, peer scans, drain coordinators, delivery
  observability layers, or per-endpoint health state on the send path.
- No new ADR may relax a SHALL in §1 without explicit operator approval.

## 6. Recommended sequence

1. Land AK.11 as-is (F1/F2 + resend retirement + doc reconciliation it
   already carries), through normal QA and PR to `integrate/phase-ak`.
2. One follow-up sprint for F3 + F4 (ingress unification + dead grammar
   deletion) — deletion-dominant, LOC-negative expected.
3. F5 ADR amendments ride whichever branch touches them last.
4. Cross-host proof: peer-pair smoke demonstrating (a) `atm send` local vs
   cross-host produce byte-identical request bodies apart from the
   provenance header, and (b) curl can complete a cross-host send using the
   documented canonical route — the original phase-entry bar.

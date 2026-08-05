# Phase AK — Cross-Host Mandate Compliance: Findings and Fix Scope

Status: DRAFT for operator review
Scope: `integrate/phase-ak` plus AK.11 candidate
`feature/pak-s11-m5-crosshost-proof` at `a412bf80`
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

## 3. AK.11 candidate review (`feature/pak-s11-m5-crosshost-proof`)

Verdict: **progressing in the right direction — continue, do not abandon.**
The committed candidate lands directly on
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

### F0 — Retain TLS quarantine (guardrail, not a defect)
`atm-peer-tls-interop` and `atm-storage/src/tls.rs` are operator-directed
quarantine, not leftover scaffolding. No branch in this fix scope (F1–F5)
may delete, shrink, or fold these into the production send path.
Accept when: both paths still exist unchanged at each fix-scope PR, the
existing boundary test forbidding daemon-edge use of the TLS material still
passes, and no PR description in this fix scope references deleting or
relocating either path. Any future change to this guardrail requires
explicit operator approval, not QA/ADR sign-off. Owner: every branch in
this fix scope; enforced at QA on each PR.

### F1 — Direct send uses the canonical singleton write  ✅ in AK.11
Accept when: the only production peer sender emits `RequestEnvelope::Write`
via the shared writer; `send_peer_http_batch` has no production caller.
Owner: AK.11 candidate `a412bf80` (needs QA and merge).

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
listener with the same result. Owner: AK.12.

### F4 — `PeerMessageArray` disposition (remaining work)
With resend retired there is no production sender of `messages[]`.
Frame as **forgot-to-delete**: remove `peer_array.rs`, the `messages[]`
branch of the peer decoder, `ApiRequest::PeerMessages` routing, the
`peer_message_array` tests, and the resend accessors left in
`peer_config_store` (beyond the compatibility `set false` surface AK.11
keeps). If recovery is ever revived, it re-enters under the original
allowance: default-OFF, one timer-driven state machine, `messages[]` to
the same endpoint — as a new approved sprint, not as retained scaffolding.
Owner: AK.12, combined with F3 and F6.

### F5 — ADR reconciliation (remaining work)
ADR-047's "the origin emits one ordered `PeerMessageArray`… a direct send
is the one-element case" and ADR-046's default-on cache language are now
wrong twice over (vs the mandate and vs AK.11). Retire ADR-046 rather than
edit it in place; its replacement must restate the constraint as a literal
checklist quoting §1's three SHALLs verbatim, not narrative prose that can
drift again. QA gates (req-qa/arch-qa) must verify against that checklist
(or this document's §1), never against ADR prose. Owner: AK.14.

### F6 — Mechanical gate for the coordinator pattern (remaining work)
Two leftover enforcement artifacts must come down with the tombstone code, not
after. The manifest is already marked retired, but its continued existence
still advertises a mechanism that this phase has removed:
- `boundaries/atm-daemon/peer-resend-scheduler.toml` — delete the manifest
  in the same commit that removes the tombstoned module. (The current
  candidate marks it retired; AK.12 deletes it instead of preserving a stale
  boundary node.)
- `crates/atm-architecture/tests/boundary_enforcement.rs::peer_resend_scheduler_direct_calls`
  (currently a bare counter, line ~926) must become a targeted forbidding
  assertion: reject the retired peer identifiers and a peer delivery router
  that dispatches through any outbound function other than the single shared
  send function. Do not use a broad generic suffix ban that can reject
  unrelated daemon internals.
Accept when: the manifest is gone, the lint is forbidding (not counting),
and a deliberately-reintroduced coordinator type fails CI.
Owner: AK.12.

### F7 — Requirements hardening (remaining work)
Add a machine-checkable sub-requirement to `docs/requirements.md` capturing
this mandate. **Use `REQ-CORE-TRANSPORT-002E`** — `002C` is already assigned
to an unrelated same-host-proof requirement (existing IDs in use: `002`,
`002A`, `002B`, `002B1`, `002C`, `002D`). Text should state the three SHALLs
of §1 plus the default-off resend allowance, in checklist form. It must also
state that the AK.11–AK.14 baseline has no automatic replay. Owner: AK.14.

## 5. Non-goals / guardrails

- **Do not delete** `atm-peer-tls-interop` or `atm-storage/src/tls.rs`
  (operator-directed quarantine of working TLS code; boundary test keeps it
  out of production).
- No re-hydration of any deleted machinery: no outbound workers, resolver
  threads, connection pools, peer scans, drain coordinators, delivery
  observability layers, or per-endpoint health state on the send path.
- No new ADR may relax a SHALL in §1 without explicit operator approval.
- **Process rule:** any future ADR touching cross-host send must quote the
  §1 checklist verbatim in its own text (not paraphrase it), and req-qa/
  arch-qa must verify implementations against that quoted checklist — never
  against ADR narrative alone. This is the concrete fix for how ADR-046/047
  let QA validate against decisions instead of the mandate.

## 6. Recommended sequence

1. Land AK.11 as-is (F1/F2 + resend retirement + doc reconciliation it
  already carries), through normal QA and PR to `integrate/phase-ak`.
2. One follow-up sprint for F3 + F4 + F6 (shared ingress, dead grammar and
   tombstone deletion, targeted anti-regression guard) — deletion-dominant,
   LOC-negative expected.
3. One physical conformance sprint proves direct singleton delivery,
   receiver-only hook behavior, duplicate idempotence, and outage/restoration
   with no automatic replay on M4↔M5 and M4↔Windows.
4. F5/F7 ADR and requirement reconciliation follows the proven final
   baseline.
5. Only after that minimal baseline is accepted, a new explicitly approved
   sprint may add the original optional extension: default-off, one bounded
   `messages[]` replay after an existing heartbeat reports an
   unavailable→healthy transition. It must preserve the same endpoint,
   shared ingress/route, and single post-persistence receive path.

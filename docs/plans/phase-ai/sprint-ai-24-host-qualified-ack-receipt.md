---
title: Sprint AI.24 — Host-qualified ACK receipt and nudge proof
---

# Sprint AI.24 — Host-qualified ACK receipt and nudge proof

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.24
worktree: ../atm-core-worktrees/feature/pAI-s24-host-qualified-ack-receipt
branch: feature/pAI-s24-host-qualified-ack-receipt
status: proposed
estimated_scope: focused end-to-end regression proof and any minimal canonical-write fix it exposes
```

## Goal

Prove that an ordinary `atm ack <message-id> <reply>` returns a reply over the
same host-qualified HTTPS write path as `atm send`, then persists that reply
and emits the recipient's normal nudge. The required proof uses the daemon's
advertised IPv4 address over its virtual-Ethernet TCP interface, the same
interface a remote host uses. `localhost` is retained only as address-grammar
coverage; neither form is a self-send exception mode or a special ACK
transport.

## Scope Summary

The controlled setup first sends an ack-required message from
`<self>@<team>` to `<self>@<team>.<advertised-ip>`. The inbound copy becomes
pending-ack. A separate grammar row parses `<self>@<team>.localhost` without
self rejection but does not count as the virtual-Ethernet transport proof. Running
the existing CLI form `atm ack <pending-message-id> <reply>` must preserve the
host-qualified source/reply target, create one canonical write containing
`acknowledges_message_id`, and return that reply through ordinary peer ingress.
The receiver must be able to read the reply by its returned ULID and must
receive exactly one configured nudge after persistence.

## Governing Requirements

- `REQ-CORE-TRANSPORT-002`: one post-write router selects local nudge for an
  empty destination host and HTTPS delivery for every present host.
- `REQ-CORE-TRANSPORT-002C`: localhost and the daemon's own advertised or
  bound IP are ordinary remote-host targets, never a loopback-only path.
- `REQ-CORE-TRANSPORT-003`: an acknowledgement is an ordinary immutable write;
  no separate ACK transport, queue, receipt, or sender-side ACK state exists.

## Governing ADRs

- `ADR-031-remote-target-contract-and-cross-host-dispatch.md`
- `ADR-034-minimal-cross-host-https-transport.md`
- `ADR-035-canonical-write-ingress-and-host-routing.md`

## Governing Boundaries

- The only ingress distinction is authenticated transport provenance:
  `Local` versus `Peer`. Both must enter `ApiRouter::route`, then the same
  `DaemonRequestDispatcher::dispatch` and canonical write handler.
- `PostWriteRouter` runs once after a new write and after the narrow
  same-store peer-receipt disposition defined below. It must emit the
  recipient nudge only after the receiver-visible row is readable by
  `atm read`.
- ACK is represented solely by `WriteRequest.acknowledges_message_id`; no
  `AckRequest` transport branch, ACK-specific peer listener, or host-specific
  nudge handler may be introduced.

## Explicit Code Samples

```rust
/// Transient result of one canonical storage operation; not durable state.
enum DuplicateWriteDisposition {
    NotDuplicate,
    AlreadyDeliveredRemote,
    SameStorePeerReceipt,
}

// Same-store peer receipt: log, skip persist_message_record, then permit only
// the inbound local-nudge post-write action. Never peer-deliver again.
```

The implementation may use a differently named private enum, but it must
express these three outcomes. A boolean such as `newly_persisted` alone is
insufficient because it cannot distinguish a normal remote replay from the
same-store peer receipt that must continue recipient notification.

## Prerequisites

- AI.22 preserves a destination host through address parsing and exempts every
  host-qualified destination from the identity-only self-send guard.
- AI.23 establishes and tests shared local/peer write ingress and provenance.
- A test daemon has one enabled local nudge target and an insecure-smoke or
  test TLS peer configuration for its advertised IPv4 address.

## Hard Dependencies

- AI.11–AI.16
- AI.22
- AI.23

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-24` and prove
  the matching client/daemon value through `atm doctor --json` before runtime
  evidence.

## Non-Goals

- Physical second-host evidence, peer authority DNS resolution, TLS hardening,
  retry/reconciliation, or a new public ACK address flag.
- Changing CLI ACK syntax. The target is derived from the pending message's
  preserved host-qualified sender/source address.
- Any loopback-only or self-IP-only production route.

## Sub-Tasks

1. **Preserve the host-qualified reply target through the canonical ACK write.**
   - Development work: in `crates/atm-core/src/ack/mod.rs`, keep
     `ReplyTarget.host` through `validate_reply_target` and
     `canonical_ack_write_request`; do not recreate a hostless `AgentAddress`.
     `validate_reply_target` first uses authenticated durable source-host
     provenance. For the narrow same-store receipt whose duplicate database
     write was intentionally skipped, it instead reads the retained origin
     destination host through one new schema accessor in
     `crates/atm-core/src/schema/inbox_message.rs`. That fallback is reply
     routing metadata only; it does not fabricate source provenance or mutate
     the stored record. The generated reply sets only
     `acknowledges_message_id` in addition to normal send fields. There is no
     `is_self_ack_reply_target` helper to add or preserve: the shared AI.22
     identity-only self-send guard is the sole suppression rule.
   - Required tests: construct a pending message whose durable source is
     `self@team.<advertised-ip>` and a same-store pending message with only
     retained origin destination metadata. Assert both ACK writes carry the
     same destination host and source message ULID. Keep one parser-only
     `localhost` case.
   - Required boundary update: extend the AI.23 structural test to reject a
     peer-facing `AckRequest`/`SendRequestEnvelope::Acknowledge` transport
     branch after this sprint.

2. **Add a reusable daemon-pair same-host ACK proof.**
   - Development work: add a test helper under
     `crates/atm-daemon/src/tests/` (or the existing peer-smoke test module)
     that uses the real local HTTP and HTTPS adapters, not direct dispatcher
     calls. It must configure one recording nudge sink for `self@team`.
   - Required tests: over the advertised-IP virtual-Ethernet target, send one
     ack-required message, read the resulting pending message, execute the
     normal CLI-equivalent ACK request, then poll
     `atm read --message-id <reply-ulid>` until it observes the reply. Assert
     the recorded nudge names that reply ULID, its sender, and
     `acknowledges_message_id`.
   - Required doc update: document this proof row in the Phase AI smoke matrix
     as a same-host prerequisite, not cross-host release evidence.

3. **Classify the same-store peer receipt without a second database write.**
   - Development work: update
     `crates/atm-core/src/send/persistence.rs`,
     `crates/atm-core/src/send/delivery_persistence.rs`, and
     `crates/atm-core/src/send/mod.rs`. When authenticated peer ingress finds
     an identical existing ULID whose record is the local origin's retained
     host-qualified outbound record, it must: log an info event, skip
     `persist_message_record`, preserve the origin record and destination-host
     metadata, and return a narrow `SameStorePeerReceipt` result. That result
     permits the ordinary inbound local-nudge post-write action but never
     re-enters peer delivery. It is an operation result, not a durable receipt,
     queue, replay, or duplicate-delivery state machine.
   - Required tests: fail if `.localhost` or `.advertised-ip` reaches a direct
     local mailbox path, if this receipt rewrites/removes the origin host
     metadata, if it writes a second record, if the receiver nudge precedes
     the readable row, or if it re-enters outbound peer delivery. Assert the
     structured info event `peer_duplicate_write_skipped` contains the ULID,
     source host, destination host, `same_store_peer_receipt=true`,
     `database_write=skipped`, and `delivery=continued`.
   - Required boundary update: add an architecture enforcement assertion that
     the peer HTTPS handler calls `ApiRouter::route(..., Peer, ...)` and that
     both peer and local adapters converge before `dispatch`.

4. **Set the sprint release identity before exercising daemon evidence.**
   - Development work: make the first commit update the workspace release
     metadata for every releasable ATM assembly to `1.3.2-beta-24`; do not
     bump CLI and daemon independently. Update `Cargo.lock` only if Cargo
     changes it.
   - Required tests: release-build `atm` and `atm-daemon`, start exactly one
     managed test daemon, and assert `atm doctor --json` reports matching
     client/daemon release `1.3.2-beta-24` before the proof begins.
   - Required doc update: retain the release label in the smoke evidence.

## Split Recommendation

Do not split the parser and advertised-IP rows: they prove one ACK invariant
through the same helper. Split immediately if fixing host preservation requires a
second write handler, a second post-write router, or a changed public ACK
syntax; that would be an architectural regression, not an AI.24 solution.

## Acceptance Criteria

- `<self>@<team>.localhost` parses and does not self-reject; it is not used as
  same-host transport evidence.
- With `ATM_IDENTITY=<self>` and `ATM_TEAM=<team>`, an ack-required send to
  `<self>@<team>.<advertised-ip>` traverses the advertised virtual-Ethernet
  TCP interface, produces a pending inbound message, and ordinary
  `atm ack <message-id> <reply>` produces one readable reply and one nudge.
- Each reply preserves the original message ULID in
  `acknowledges_message_id`; the reply's ULID is new, immutable, and visible
  in the receiver inbox before its nudge is emitted.
- The source pending-ack message changes to acknowledged only through the
  shared receive-side `WriteRequest { acknowledges_message_id: Some(..) }`
  handling; the proof reads that state after the reply is visible and rejects
  any sender-side or ACK-private state mutation.
- A host-qualified source address is never treated as a historical self-ACK
  suppression target: `atm ack` emits its ordinary reply write and nudge.
- Local CLI, loopback HTTP, and HTTPS peer ingress converge at
  `ApiRouter::route` and one canonical write handler. No ACK-specific
  transport/persistence/nudge branch exists.
- For the advertised-IP host-qualified self route, peer ingress finds the
  origin ULID, logs the duplicate write attempt as skipped/continued, retains
  one database record with its original destination-host metadata, performs no
  second database write or peer re-delivery, and still emits the terminal
  recipient nudge. A later ordinary `atm ack` reload derives its reply target
  from that retained destination host and emits the same canonical
  `WriteRequest { acknowledges_message_id: Some(..), .. }`. A conflicting
  immutable payload remains a typed conflict with no nudge.
- The build fails if the advertised-IP target bypasses peer ingress, nudge
  precedes persistence, the duplicate receipt has no terminal nudge, or it
  mutates/removes the origin destination-host metadata.
- `atm doctor --json` reports client and daemon `1.3.2-beta-24` before the
  proof, and the evidence records the one daemon PID and release values.
- An independent quality review runs the release-built branch daemon and the
  real CLI advertised-IP ACK scenario. Its retained evidence must include the
  duplicate-write-skipped/continued log row, readable reply ULID, and terminal
  nudge; direct-dispatch or mocked-nudge evidence cannot close this sprint.

## Required Validation

- `just lint`
- `just test`
- `cargo build --release --bin atm --bin atm-daemon`
- The advertised-IP daemon-pair proof above, with sanitized retained logs and
  `git diff --check`
- Switch the CLI and daemon together with `daemon-switch`, retain one managed
  branch daemon for the proof, and leave it running for independent quality
  review after the evidence is captured.

## Required Document Updates

- `docs/requirements.md` and `docs/atm-core/requirements.md`: state that an
  ACK reply to a host-qualified source is an ordinary canonical write and is
  covered by the same-host remote-target contract.
- `docs/adr/ADR-035-canonical-write-ingress-and-host-routing.md`: add the
  host-qualified ACK proof to the canonical-write verification inventory.
- `docs/plans/phase-ai/plan-phase-ai.md` and `README.md`: list AI.24 and its
  release candidate.

## Risks And Watchouts

- A green sender-side ACK result is not receipt evidence. The test must prove
  the reply ULID in the receiver inbox and the corresponding receiver nudge.
- Do not treat a raw TCP connection, source-host provenance alone, or a
  recording fake transport as proof of the HTTP adapter path.
- The peer adapter may normalize destination routing metadata only after
  authentication. It must preserve source provenance and the ACK reference.

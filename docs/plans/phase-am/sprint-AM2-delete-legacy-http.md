# AM.2 — Delete Shared Raw HTTP Framing

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.9 proof/ledger acceptance and AM.1's accepted frozen
ledger. Both parent PRs must be merged before this deletion PR begins; follow
the ledger topology rather than the numerical labels.
**unblocks:** AM.3 and AM.4.
**parallel_safe:** none; deletion touches shared transport composition.

**traceability:** `REQ-CORE-TRANSPORT-006`,
`REQ-DAEMON-TRANSPORT-006/007`, ADR-033, ADR-036.

## Deliverables

- Delete `HttpFrameReader`, handwritten request/response framing helpers, and
  their direct tests/exports after AL.9 establishes framework HTTP.
- Remove only the Cargo dependencies/docs that belong exclusively to those
  helpers; leave local, peer, and replay deletion to their dedicated sprints.
- Enable the raw-framing negative guard from AM.1 in the same deletion PR.

## Paths/categories to delete

- `HttpFrameReader` and ATM-owned HTTP header/body/frame parsing or writing.
- The direct tests, exports, documentation, and Cargo dependencies belonging
  solely to shared raw framing.

The frozen ledger, not this list or the numeric sprint label, is authoritative
for paths and order at implementation time.

## Acceptance criteria

- Repository guards find no active raw ATM HTTP parser/writer.
- Every already-migrated client/listener still uses AL's shared typed client
  and one typed handler.
- Local and M5 smoke pass after deletion.

## Required validation

- compilation/test/format/lint suite
- negative-symbol architecture guards
- focused framework-route and client integration tests plus full test suite

## Non-closure

Local/peer adapter and recovery/replay deletion are intentionally separate in
AM.3–AM.5; AM.2 does not hide those survivors behind a compatibility wrapper.

# AM.2 — Delete Shared Raw HTTP Framing

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.9 proof/ledger acceptance, AM.1's accepted frozen ledger,
and AM.3's local-listener deletion merged forward into this branch.  AM.2 may
build on AM.3's pushed branch before its PR merge.  AM.2 owns the remaining
RM-002 non-write compatibility-client migration and the typed-runtime request
conversion before deleting their RM-001 raw-framing callee.
**unblocks:** AM.4.
**parallel_safe:** none; deletion touches shared transport composition.

**traceability:** `REQ-CORE-TRANSPORT-006`,
`REQ-DAEMON-TRANSPORT-006/007`, ADR-033, ADR-036.

## Deliverables

- Migrate the retained `atm-daemon-client` bootstrap/non-write
  read/ack/admin compatibility exchange to the shared typed HTTP client,
  without changing its public compatibility contract or routing writes through
  it.
- Move `atm-http-runtime`'s `HttpRequest` → `ApiRequest` conversion from the
  core raw-framing helper into the typed runtime boundary.
- Delete `HttpFrameReader`, handwritten request/response framing helpers, and
  their direct tests/exports only after those migrations make the raw-framing
  inventory empty.
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
- The retained non-write compatibility client and every runtime listener use
  AL's shared typed client/handler path, with no raw-frame helper.
- Local and M5 smoke pass after deletion.

## Required validation

- compilation/test/format/lint suite
- negative-symbol architecture guards
- focused framework-route and client integration tests plus full test suite

## Non-closure

Local/peer adapter and recovery/replay deletion are intentionally separate in
AM.3–AM.5.  AM.2 may retain the non-write compatibility API, but must replace
its raw-framing implementation rather than hide a second transport behind it.

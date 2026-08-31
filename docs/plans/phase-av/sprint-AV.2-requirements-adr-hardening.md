---
phase: AV
sprint: AV.2
title: Requirements and ADR hardening for read concurrency
branch: docs/av2-read-concurrency-requirements
integration_branch: integrate/phase-av
status: planned
recommended_agent: Cipher-311d
recommended_model: fast
dependency_relations:
  - related: AV.1
    relation: parallel_safe
    rationale: AV.2 edits docs/requirements.md and docs/adr/ only; AV.1 edits
      crates only. No file, contract, or artifact intersection.
  - related: AV.3
    relation: parallel_safe
    rationale: AV.3 edits tests/lint tooling; AV.2 edits normative docs. The
      ADR here cites gate names but does not depend on their landing order.
---

# AV.2 — Requirements and ADR hardening for read concurrency

Make the pre-AV mode of operation normatively non-compliant: codify the
read-concurrency contract and the race-tolerant state semantics so a
future change cannot re-fence reads "for safety" without violating a
written requirement.

## Deliverables

This is the authoritative deliverable checklist.

- [ ] D1 — `docs/requirements.md` amendments (normative MUST language):
      read-family operations (read/peek/list/doctor/query) MUST be
      serviced concurrently; MUST NOT share a concurrency bound with, or
      be ordered behind, any write or housekeeping lane; read deadlines
      MUST be enforced (cancellable); bounded overload MUST fail
      explicitly.
- [ ] D2 — Race-tolerance codified in `docs/requirements.md`: primary
      message records are immutable; mutable state (read/ack/seen) is
      race-tolerant — a read racing a state change may return either
      value; consequently no requirement may demand read-your-writes,
      snapshot pinning, or reader/writer fencing on mailbox reads.
- [ ] D3 — New ADR `docs/adr/` (next free number): reader/writer lane
      architecture — bounded RO WAL reader pool, ordered writer lane
      scoped to durable admission + state transitions, deadline
      semantics split (reads cancellable, writes run-to-completion),
      with the AL3→AL13-G7 single-permit regression recorded as
      motivating history (phase-av-plan.md §1.1).
- [ ] D4 — Phase-AM deletion ledger updated: `BlockingCoreBridge` and
      sync read-bridge remnants added as deletion targets with their
      file paths.

## Acceptance criteria

This is the authoritative acceptance checklist.

- [ ] A1 — Requirements text uses testable MUST/MUST NOT statements; no
      aspirational "should" for the lane-separation rules.
- [ ] A2 — The ADR names the concrete types/modules (BlockingCoreBridge,
      WriteOp, AsyncMailboxReader) and cross-references the AV.3 gates
      that enforce it.
- [ ] A3 — No contradiction with ADR-036 storage topology or the
      Phase-AM deletion plan; ledger entries point at real paths.
- [ ] A4 — D2 language matches the phase plan §1.2 contract verbatim in
      substance (either-value race outcome, zero writer-lane
      coordination).

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint` (doc checks)
- [ ] Cross-reference check: every file path cited in the ADR and
      ledger entries exists at the cited revision.
- [ ] quality-mgr doc review PASS.

## Out of scope

- Any code or test change — AV.1/AV.3.
- Benchmark documentation — AV.4.

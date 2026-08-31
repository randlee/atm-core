---
phase: AV
sprint: AV.1b
title: Read-handler cutover and writer purity
branch: feature/av1b-read-handler-cutover
integration_branch: integrate/phase-av
stack_parent: fix/mailbox-read-blocking-serialization (AV.1a)
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1a
    relation: must_follow
    rationale: Consumes the AsyncMailboxReader capability and reader pool
      delivered by AV.1a. Stacked on AV.1a's branch; restack before every round.
  - related: AV.2
    relation: parallel_safe
    rationale: AV.2 edits docs/requirements.md and ADR files only; this
      sprint edits crates only.
  - related: AV.3
    relation: must_follow
    rationale: AV.3's gates assert the post-cutover state delivered here
      (bridge off read paths, WriteOp pure). AV.3 sits above this branch in the
      stack; restack propagates changes.
  - related: AV.4
    relation: must_follow
    rationale: AV.4 benchmarks drive the cutover read path and consume the
      AV.1a metrics seams through it.
---

# AV.1b — Read-handler cutover and writer purity

The atomic behavior change of the phase: flip the read-family handlers
onto the AV.1a reader lane, split the hidden read-flow mutations onto the
writer lane, decompose doctor, and remove the writer-queued read path.
These land together deliberately — cutting handlers over (D1) without the
mutation split (D2) and writer purity (D4) would leave reads racing a
still-present writer-lane read path; this sprint doc is the recorded
atomicity rationale. Evidence base:
[phase-av-plan.md](./phase-av-plan.md) §1.

## Deliverables

This is the authoritative deliverable checklist. Every listed
deliverable is expected to land at a production-ready level for the
scope this sprint claims; partial or shape-only completion fails the
sprint.

- [ ] D1 — Read-family handler cutover in
      `storage_and_nudge_router.rs`: list (:493-511), peek (:514-533),
      read (:536-555), doctor (:579-637) serve from the AV.1a reader
      lane; none acquires the `BlockingCoreBridge` permit.
- [ ] D2 — Hidden-mutation split: `apply_display_mutations_to_store`
      (`atm-core/src/read/mod.rs:354-365`) and the seen-watermark write
      (:211-225) become explicit writer-lane state transitions enqueued
      after read-only selection returns (race-tolerant per phase plan
      §1.2).
- [ ] D3 — Doctor decomposition: core doctor projection
      (`doctor/mod.rs:130-170,173-230`) is an async, independently
      bounded control-plane composition; Herdr-presence leg stays
      separately timed; doctor acquires neither reader-pool permits nor
      the writer lane.
- [ ] D4 — Writer purity: `WriteOp::ListMessages`
      (`writer/ops.rs:37,106`), `SharedDb::submit_list_messages_async`
      (`shared_db.rs:482-501`), and the writer-routed
      `list_messages_async` delegation (`lib.rs:612-615`) removed.
- [ ] D5 — Deterministic liveness tests in the router fixture
      (`storage_and_nudge_router.rs:1053+`): stalled housekeeping seam +
      concurrent read storm within budget; bounded-overload explicit
      failure. Runs in standard `just test`.

## Code contracts

```rust
// Writer-lane state transition replacing the hidden read-flow mutation
// (D2). Enqueued after the read returns; loss/reorder races are
// acceptable per the race-tolerant state contract.
// (Extends the existing WriteOp enum in writer/ops.rs.)
WriteOp::ApplyReadDisplayState {
    mailbox: MailboxId,
    message_ids: Vec<MessageId>,
    seen_watermark: Option<Watermark>,
}
```

## Acceptance criteria

This is the authoritative acceptance checklist (phase contract points
1–6 mapped to testable statements).

- [ ] A1 — No mailbox read/peek/list/doctor path references
      `BlockingCoreBridge`, `spawn_blocking`, or sync `*_with_runtime`
      read APIs (verified by grep + architecture test).
- [ ] A2 — Liveness test: with one housekeeping/mutation job stalled and
      writer activity running, ≥10 concurrent list/peek/read/doctor
      calls across distinct teams each complete within their request
      budget.
- [ ] A3 — Overload test: reads beyond pool + queue capacity fail
      explicitly with `Saturated`/`DeadlineExpired`, not by queuing
      indefinitely.
- [ ] A4 — Read flows perform zero writer-lane work before returning;
      display/seen mutations are observed on the writer lane afterward.
- [ ] A5 — Doctor completes while both the reader pool and writer lane
      are saturated.
- [ ] A6 — `WriteOp` contains no pure-read variant; the writer queue
      receives no read traffic under a read-only workload (asserted via
      writer metrics in a test).
- [ ] A7 — All existing mailbox read/clear/graft behavior tests pass
      unchanged except where they asserted serialization.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`
- [ ] `just test` (includes D5 liveness tests)
- [ ] `just validate`
- [ ] Architecture/boundary tests green (`cargo test -p atm-architecture`)
- [ ] Live manual proof on a local daemon build: `atm read` under
      induced housekeeping stall returns within budget (gate feature —
      live proof before QA dispatch).

## Out of scope

- Reader pool/capability internals — AV.1a.
- Deleting `BlockingCoreBridge` remnants used by mutation paths and the
  enforcement gates — AV.3.
- Requirements/ADR text — AV.2. Benchmarks — AV.4.
- Any change to the frozen legacy synchronous daemon.

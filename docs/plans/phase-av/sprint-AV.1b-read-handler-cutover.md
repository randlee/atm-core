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
      read (:536-555), doctor (:579-637) route through the AV.1a D6
      `AsyncMailboxRuntime` port (which composes the reader lane, the
      extracted pure selection module, and the writer-lane state
      handoff); none acquires the `BlockingCoreBridge` permit, and no
      handler re-implements selection/authorization logic inline. The
      port preserves today's team/agent authorization and visibility
      filters; the AV.1a A6 parity suite (read/peek/list/missing-record/
      state-transition) is extended here to run through the live
      handlers. The frozen legacy synchronous daemon is not modified.
- [ ] D2 — Hidden-mutation split: `apply_display_mutations_to_store`
      (`atm-core/src/read/mod.rs:354-365`) and the seen-watermark write
      (:211-225) become explicit writer-lane state transitions governed
      by exactly one handoff protocol (below); there is no other
      permitted mutation path from a read flow.

      **Mutation-handoff protocol (normative for this sprint):**
      1. *Response eligibility:* the read response is eligible to return
         as soon as read-only selection completes. It NEVER awaits
         writer-lane *execution* or durability.
      2. *Admission:* before returning, the read flow awaits only
         **enqueue admission** — a bounded, non-blocking-in-practice
         `try_enqueue` onto the writer ingress. Admission either
         succeeds immediately or fails immediately; there is no
         unbounded wait on writer capacity in the read path.
      3. *Admission failure:* if the writer ingress is saturated or
         unavailable, the state transition is **dropped, counted, and
         logged** (a `read_state_handoff_dropped` metric with reason
         saturated/unavailable) and the read response still returns
         success. Dropping is authorized ONLY at this admission point —
         a transition that was admitted is executed or fails loudly
         through normal writer-lane error handling; silent loss after
         admission is not race-tolerance, it is a bug.
      4. *Permitted races:* once admitted, ordering/visibility races
         with concurrent reads are "don't care" per phase plan §1.2. No
         retry loop in the read path; observability (metric + log) is
         the recovery surface.

      Tests: writer-lane saturated at admission (read succeeds, drop
      counted); writer-lane execution failure after admission (error
      surfaced via writer-lane error path/metrics, read unaffected);
      response-before-commit (read returns before the admitted
      transition is applied; a subsequent read observes it
      eventually).
- [ ] D3 — Doctor decomposition: a typed `DoctorProjection` boundary
      owned by `atm-runtime` (beside the AV.1a D6 port) replaces the
      bridged sync doctor call. Each source leg is enumerated with its
      owning layer, lane, and its own deadline:
      | Leg | Owner | Lane |
      |---|---|---|
      | core doctor projection (`doctor/mod.rs:130-170,173-230`) | atm-core via a dedicated bounded control-plane worker | control lane (own bound) |
      | roster/lease validation | runtime roster component | control lane |
      | peer-config / runtime-health | runtime config/health components | in-process async (no storage) |
      | Herdr presence | herdr client | own timeout, already separately timed |
      Legs run under bounded structured concurrency (join with per-leg
      deadline; one slow leg degrades its own section, never the whole
      report). Doctor acquires neither mailbox reader-pool permits nor
      the writer lane; its control lane has its own explicit bound.
      Test: saturate a doctor control-plane dependency while BOTH the
      mailbox reader pool and the writer lane are saturated — doctor
      returns a within-deadline report with the slow leg marked
      degraded, and mailbox reads remain unaffected.
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
// (D2). Handoff follows the D2 protocol: response is eligible on
// selection completion; only bounded enqueue ADMISSION is awaited;
// admission failure drops-with-count; post-admission loss is a bug.
// (Extends the existing WriteOp enum in writer/ops.rs.)
WriteOp::ApplyReadDisplayState {
    mailbox: MailboxId,
    message_ids: Vec<MessageId>,
    seen_watermark: Option<Watermark>,
}

// D2 admission surface on the writer ingress (indicative):
pub enum HandoffAdmission {
    Admitted,
    Rejected(HandoffRejection),   // Saturated | Unavailable — counted + logged,
}                                 // read response returns success regardless
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
- [ ] A4 — Read flows perform no writer-lane *execution* work before
      returning (bounded enqueue admission per the D2 protocol is the
      only writer-lane touch); display/seen mutations are observed on
      the writer lane afterward. D2 protocol tests pass: saturation
      drop-with-count, post-admission writer failure surfaced loudly,
      response-before-commit.
- [ ] A5 — Doctor completes within deadline while both the mailbox
      reader pool and writer lane are saturated, including the D3 case
      where a doctor control-plane dependency is also saturated (slow
      leg reported degraded, report still returned).
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

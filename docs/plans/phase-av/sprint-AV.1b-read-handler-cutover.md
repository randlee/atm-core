---
phase: AV
sprint: AV.1b
title: Read-handler cutover and writer purity
branch: feature/av1b-read-handler-cutover
integration_branch: integrate/phase-av
stack_parent: fix/mailbox-read-blocking-serialization (AV.1a) — planned; stack provisioned by task AV.0 (phase plan §4)
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
      read (:536-555) route through the AV.1a D6 `AsyncMailboxRuntime`
      port (which composes the reader lane, the extracted pure selection
      module, and the writer-lane state handoff); doctor (:579-637)
      routes through the separate async `DoctorProjection` port (D3) —
      this exact split (mailbox family → `AsyncMailboxRuntime`; doctor →
      `DoctorProjection`) is the one architecture, asserted by A1 and
      the AV.3 D2 allowlist. None acquires the `BlockingCoreBridge`
      permit, and no handler re-implements selection/authorization
      logic inline. The
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
      2. *Supervised handoff (non-blocking, durable-in-process):* the
         read flow hands the transition to a **state-handoff
         supervisor** — a dedicated async task owned by the
         `AsyncMailboxRuntime` composition with its own bounded buffer
         (`handoff_buffer` config). The read path performs only a
         synchronous `try_push` into that buffer; it NEVER awaits writer
         admission, writer capacity, or writer execution. The
         supervisor, not the read path, awaits writer-ingress admission
         with its own deadline and retries with backoff while the
         writer lane is saturated or temporarily unavailable.
      3. *Loss is not authorized by race tolerance.* Race tolerance
         (§1.2) governs what a concurrent read may *observe*; it does
         not license discarding a requested transition. A transition may
         be lost in exactly two circumstances, both explicit: (a) the
         supervisor buffer is full at `try_push` (counted as
         `read_state_handoff_rejected`, logged, and surfaced as a
         degraded section in doctor); (b) process exit with unapplied
         buffered transitions. In both cases the **end-user semantic is
         fail-safe: the message simply remains unread/unseen and is
         presented again on the next read** — a message is never hidden
         or lost. This semantic is recorded as a normative requirement
         by AV.2 (`R-STATE-HANDOFF-1`) and in the AV.2 ADR; it is the
         product decision this sprint implements, not an implementation
         convenience.
      4. *Permitted races:* once buffered, ordering/visibility races
         with concurrent reads are "don't care" per phase plan §1.2.
      5. *Supervisor lifecycle and fault contract:* the supervisor task
         is owned by the `AsyncMailboxRuntime` composition, which holds
         its monitored `JoinHandle`. **Startup readiness:** handlers do
         not admit reads until the supervisor reports ready; failed
         startup fails runtime construction (fail closed). **Task
         fault (error/panic/cancellation):** the runtime atomically
         flips the handoff to `Unavailable` — new `try_push` calls are
         rejected (counted as `read_state_handoff_rejected{reason=
         unavailable}`, doctor reports the handoff leg degraded) while
         reads keep returning — the buffer is retained, and the runtime
         restarts the supervisor (bounded attempts, `supervisor_max_
         restarts`) which drains the preserved buffer. Restart-budget
         exhaustion is a runtime fault: the runtime fails closed (this
         *is* the process-exit loss case, not a third one). **Permanent
         writer failure:** each transition carries a retry deadline
         (`handoff_retry_deadline`, backoff-bounded); exhaustion means
         the writer lane is permanently failed, which is likewise a
         runtime fault → fail closed. A stalled retry loop is detected
         by the same deadline. Metrics: supervisor state gauge
         (ready/unavailable/restarting), restart count, retry-deadline
         exhaustions, buffered depth.

      Tests: writer ingress saturated (read returns within budget,
      supervisor applies the transition once capacity frees); writer
      lane unavailable then recovers (transition applied after recovery,
      read unaffected); supervisor buffer full (explicit rejection
      counted, read still succeeds, message still unread on the next
      read); process restart with buffered transitions (messages
      re-presented as unread, none missing); response-before-commit
      (read returns before the transition is applied; a later read
      observes it); supervisor failed startup (runtime construction
      fails, no reads admitted); forced supervisor exit with queued
      transitions (handoff flips Unavailable, reads still succeed within
      budget, restarted supervisor drains the preserved buffer, none
      lost); restart-budget exhaustion and permanent writer failure
      (runtime fails closed, never silent success with stranded
      transitions).
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
      the writer lane. Contract (indicative; see Code contracts):

      ```rust
      #[async_trait]
      pub trait DoctorProjection: Send + Sync {
          async fn project(&self, scope: DoctorScope, deadline: RequestDeadline)
              -> Result<DoctorReport, DoctorError>;
      }
      pub struct DoctorReport { pub legs: Vec<LegResult> /* one per leg */ }
      pub enum LegResult { Ok(LegReport), Degraded { leg: LegId, reason: LegDegraded } }
      pub enum LegDegraded { DeadlineExpired, Unavailable(String) }
      pub enum DoctorError {              // whole-request outcomes only
          Saturated { pool_size: NonZeroUsize, queue_depth: usize },
          DeadlineExpired { waited: Duration },
      }
      ```
      Per-leg degradation is a *successful* report with `Degraded`
      entries; `DoctorError` is reserved for whole-request control-lane
      saturation or overall deadline expiry. Tests cover both classes
      (per-leg degraded assembly; whole-request `Saturated`; overall
      deadline) plus live-handler parity against the current doctor
      output for the healthy case.

      **Control-lane capacity (normative):** the core-doctor leg runs on
      a bounded **multi-worker** control lane — a third instance of the
      AV.1a D2 pool type — `doctor_pool_size` workers (default 4) each
      with its own RO connection, a bounded queue (`doctor_queue_depth`,
      default 16; both knobs in the shared `[reader_lanes]` section and
      counted in the AV.1a D2 connection budget), and per-request
      deadline; beyond pool + queue, doctor fails explicitly with
      `Saturated`, never queues indefinitely. Rationale for the bound:
      doctor is low-frequency control-plane traffic, and the bound
      exists to protect the database from a doctor storm — it is
      resource management, not serialization; a single-worker lane is
      explicitly non-compliant (it would relocate the one-permit
      regression). Tests: (i) ≥8 concurrent doctor calls across distinct
      teams complete within budget while one doctor control-plane
      dependency is saturated and BOTH the mailbox reader pool and the
      writer lane are saturated — each report returns within deadline
      with only the slow leg marked degraded; (ii) one deliberately slow
      doctor request delays neither an independent doctor request nor an
      independent mailbox read beyond its budget.
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
// selection completion; the read path only try_pushes into the
// supervisor buffer; the supervisor owns writer admission + retry.
// (Extends the existing WriteOp enum in writer/ops.rs.)
WriteOp::ApplyReadDisplayState {
    mailbox: MailboxId,
    message_ids: Vec<MessageId>,
    seen_watermark: Option<Watermark>,
}

// D2 supervisor surface (indicative), owned by the AsyncMailboxRuntime
// composition. try_push is synchronous and never awaits the writer.
pub struct StateHandoffSupervisor { /* bounded buffer + monitored retry task */ }
impl StateHandoffSupervisor {
    pub async fn start(cfg: HandoffConfig, ingress: WriterIngress) -> Result<Self, HandoffStartupError>; // readiness-gated
    pub fn try_push(&self, op: WriteOp) -> Result<(), HandoffRejected>; // BufferFull | Unavailable
    pub fn state(&self) -> SupervisorState;                              // Ready | Unavailable | Restarting
}
pub struct HandoffConfig { handoff_buffer: usize, handoff_retry_deadline: Duration, supervisor_max_restarts: u32 }
```

## Acceptance criteria

This is the authoritative acceptance checklist (phase contract points
1–6 mapped to testable statements).

- [ ] A1 — list/peek/read handlers depend only on `AsyncMailboxRuntime`
      and the doctor handler only on `DoctorProjection` (the D1 split);
      no mailbox read/peek/list/doctor path references
      `BlockingCoreBridge`, `spawn_blocking`, or sync `*_with_runtime`
      read APIs (verified by grep + architecture test).
- [ ] A2 — Liveness test: with one housekeeping/mutation job stalled and
      writer activity running, ≥10 concurrent list/peek/read/doctor
      calls across distinct teams each complete within their request
      budget.
- [ ] A3 — Overload test: reads beyond pool + queue capacity fail
      explicitly with `Saturated`/`DeadlineExpired`, not by queuing
      indefinitely.
- [ ] A4 — Read flows perform zero writer-lane work before returning
      (the only touch is a synchronous `try_push` into the supervisor
      buffer); display/seen mutations are observed on the writer lane
      afterward. All D2 protocol tests pass: writer saturation,
      writer unavailable-then-recovers, buffer-full explicit rejection
      with fail-safe unread semantic, restart re-presentation,
      response-before-commit, and the D2.5 lifecycle cases (failed
      startup, forced supervisor exit with preserved buffer, restart
      exhaustion / permanent writer failure fail-closed).
- [ ] A5 — Doctor fan-out liveness per D3: ≥8 concurrent cross-team
      doctor calls complete within deadline while the mailbox reader
      pool, the writer lane, and one doctor dependency are all saturated
      (slow leg degraded, reports still returned); a slow doctor delays
      neither another doctor nor a mailbox read; doctor beyond control
      lane capacity fails explicitly.
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
- Renaming/narrowing the residual `BlockingCoreBridge` (eight non-read
  callers: deferred marker, clear, heartbeat, queue-get-next, graft
  receiver ×4) and the enforcement gates — AV.3; migrating those callers
  off the bridge — follow-up `AV-FU-1`. `clear_messages` stays on the
  bridge this sprint (A7 covers its behavior parity).
- Requirements/ADR text — AV.2. Benchmarks — AV.4.
- Any change to the frozen legacy synchronous daemon.

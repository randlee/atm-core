# ADR-054 — Nudge Taxonomy And Queue Mechanism

| Field | Value |
| --- | --- |
| ID | ADR-054 |
| Status | Accepted |
| Scope | `atm-storage`/`atm-core` nudge dispatch contract, `atm queue` CLI verb |
| Relates to | ADR-001, ADR-018, ADR-019, ADR-024, ADR-036, `docs/plans/phase-aq/sprint-AQ1-queue-cli.md`, `docs/plans/phase-aq/aq1-blueprint.md` |

## Context

Prior to Sprint AQ1, "nudge" named exactly one thing: the synchronous,
immediate, post-persistence recipient notification fired from
`storage_and_nudge_router.rs` and dispatched through
`MessageReceivedHookEmitter`/`AsyncMessageReceivedHookEmitter`. Sprint AQ1
adds a second, deferred kind — a message that is durably readable
immediately but whose recipient-side notification is intentionally withheld
until a later trigger (idle-transition drain, recovery sweep, Herdr pump).
Using "nudge" for both without a taxonomy makes every future reader guess
which kind a given "nudge" identifier means, and the phase-ao2 sweep that
would otherwise rename every touched identifier in one pass is too wide to
land inside one shared worktree without multiplying merge conflicts across
three concurrent lanes (§2.0 of the AQ1 blueprint).

Sprint AQ1 is also the trait-change sprint for the whole nudge/queue family:
Herdr (AQ2.6/AQ2.7), graft queue-channel delivery (AQ1.5–AQ1.9, AQ2), the
delivery-trigger work (AQ2.5), and the recovery/idle drain (AQ3) all
implement contracts this ADR closes; none of them may widen or redefine
them. This ADR is the single place those contracts are decided with
rationale, so later sprints are implementers, never definers (per the
2026-08-26 re-scope in the sprint doc).

## Decision

### (a) Taxonomy

**Nudge** is the umbrella term for post-delivery recipient notification.
It has exactly two kinds:

- **steer** — the existing immediate, synchronous, post-persistence
  notification (today's only behavior, unchanged).
- **queue** — a deferred nudge: the message is durably readable immediately;
  its recipient-side notification is withheld until a later trigger owned by
  a downstream sprint (AQ2/AQ2.5/AQ2.6/AQ3).

`NudgeKind::{Steer, Queue}` (`atm-core::boundary`) is the kind carried on
`BuiltInPostSendDispatch`; `NudgeMode::{Immediate, Deferred}`
(`atm-core::send`, caller-owned per ADR-019) is the write-time policy input
that produces it. This is disambiguated from, and does not collide with,
Hermes's session-dispatch `mode="queue"|"steer"`: Hermes's field describes
how a *session* receives a nudge already selected by this taxonomy, not a
second nudge-kind vocabulary. The two must not be merged into one enum; a
Hermes session mode is downstream of, not a synonym for, `NudgeKind`.

### (b) `nudge_pending_at` column, derived FIFO, atomic claim

`mail_message_states` gains `nudge_pending_at TEXT NULL` and
`nudge_attempts INTEGER NOT NULL DEFAULT 0` via `ensure_column` (no new
migration table). A queued message sets `nudge_pending_at` at write time;
FIFO order is *derived*, never cached: `ORDER BY message_key` over rows with
`nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL`. Message
keys are `"atm:" + ULID` (`contract.rs:48-53`), so lexicographic
`message_key` order **is** ULID order — no new sort column, and the FIFO
order survives daemon restart because it is recomputed from durable state,
never held in memory. The atomic claim is one conditional
`UPDATE … RETURNING` (`claim_next_pending`); `None` means either nothing is
eligible or another caller won the race, and callers must treat the two
identically.

### (c) Steer-suppression seam

The steer-suppression seam is caller-owned in
`PreparedWrite::build_received_hook_dispatches` (`atm-core/src/send/mod.rs`),
per ADR-019. The first statement checks `NudgeMode::Deferred` and returns
`Ok(Vec::new())` — the same "no dispatch" path the function already used for
zero built-in targets, so no new empty-vector case is introduced. Untouched
by this ADR: the router call site (`storage_and_nudge_router.rs:538`), its
`newly_persisted` guard, the `al3_*` boundary-enforcement literal
assertions, and `boundaries/atm-http-runtime/http-runtime.toml:41`'s
unconditional post-write invariant. That invariant is **closed, not
reopened**: a deferred write still runs the same unconditional post-write
step, which now legally yields an empty dispatch vector
(`send/mod.rs:397`) instead of a populated one. No manifest edit records a
behavior that was already legal before this ADR.

### (d) `PendingNudgeStore` governance

`PendingNudgeStore` is a new optional capability trait on the `atm-storage`
shared contract. ADR-018 §3 ("Storage Traits Are Semantic CRUD Traits")
caps optional capability traits at four before a follow-up ADR is required;
this ADR **is** that follow-up. It amends ADR-036's storage-boundary
inventory to add `PendingNudgeStore`, is recorded machine-readably in
`boundaries/atm-storage/pending-nudge-store.toml`, and is enforced by
`crates/atm-architecture/tests/pending_nudge_store_boundary.rs`. Per the
sprint doc's dependency ordering, `boundary-guard` review is a **merge
precondition only** — it runs in parallel with dev/QA on deliverables 2–5 and
never blocks or invalidates already-tested work in this shared worktree.

### (e) `MemberStateTransitionSink` / `RuntimeHealth` scope

Out of scope for AQ1 by design. The "stuck" health signal in (f) and any
`MemberStateTransitionSink` wiring are AQ3 deliverables; this ADR only
reserves the vocabulary (`nudge_attempts` as the single owner of retry
state for every recipient kind, `list_pending_members` as the discovery
seam) so AQ3 does not need a second trait.

### (f) Graft dual-channel contract + bounded re-attempts

`MAX_NUDGE_ATTEMPTS: u32 = 5` (`atm_storage::contract`) is the concrete max
auto-retry count for a failed deferred-nudge handoff. `requeue_pending`
increments `nudge_attempts` on every failed dispatch; at or above the max,
`claim_next_pending`'s `WHERE … AND nudge_attempts < ?` predicate excludes
the row from further auto-claim — the marker stays set (the message is
still known-pending) but becomes auto-retry-ineligible, and AQ3's recovery
sweep surfaces a distinct "stuck" signal for operator action.
`release_pending` is the separate path for a claim refused for a lifecycle
reason (AQ2.7's `agent_blocked`): it restores the marker without
incrementing `nudge_attempts`, because no delivery was actually attempted.
The graft dual-channel contract itself (steer channel vs. queue-kind wire
handoff) is implemented in AQ2/AQ1.5–AQ1.9; this ADR fixes only the shared
retry-budget contract those channels must use.

### (g) Rename/compat policy

The following are explicitly **not** renamed and are a distinct mechanism
from the nudge taxonomy in (a):

- `.atm.toml`'s `post_send_hooks` key and the external command-hook system.
- The `NudgeTemplateOverrideStore` cluster (`BuiltInNudgeTemplateKind`,
  `TeamNudgeTemplateOverrideRow`, etc.) — already umbrella-sense, keeps its
  names.
- Wire-crossing contracts: `GraftPostSendRequest`/`Response` loopback TCP
  (the receiver process can lag/lead the daemon) and the
  `ATM_INTERNAL_NUDGE`/`InternalNudgeEnvelope` env payload. Either changes
  only with an explicit both-sides (daemon + receiver process) plan.
- `PyNudge` and the Python callback shape (hermes-atm) — a future rename is
  a deprecation shim, not a breaking rename, and is out of AQ1 scope.
- `atm doctor --json` field names.

## AQ1 trait-foundation record

The sprint doc's "Trait-foundation scope" section makes the following
AQ1-only decisions binding on every downstream sprint; they may only be
implemented, never widened or redefined, without a new ADR.

### Crate placement of `MemberKey` + `PendingNudgeStore` (D1)

Evidence: `atm-core` depends on `atm-storage`
(`crates/atm-core/Cargo.toml:23-24`), but `atm-storage-rusqlite` — the crate
that must implement `PendingNudgeStore` — depends on `atm-storage` **only**
(`crates/atm-storage-rusqlite/Cargo.toml:18-19`), not `atm-core`. Putting the
trait in `atm-core::boundary` (rejected option (ii)) would force an
`atm-storage-rusqlite -> atm-core` edge, a documented forbidden edge in both
`boundaries/atm-core/message-received-hook-emitter.toml` and
`boundaries/atm-storage/nudge-template-override-store.toml`. Decision:
`MemberKey`, `NudgeClaim`, and `PendingNudgeStore` all live in
`atm-storage` (option (i)) — `MemberKey` beside `TeamName`/`AgentName` in
`atm-storage::types`, the trait beside `RosterStore`/`MessageStore` in
`atm-storage::contract`, sealed via the existing
`atm_storage::contract::sealed::Sealed`. `atm-core::boundary` re-exports all
three. `MemberKey` is distinct from the private
`atm_http_runtime::runtime_health::MemberKey`; consolidating the two is a
non-blocking follow-up, not an AQ1 deliverable.

### `mark_pending` and the crash window

The sprint doc's `PendingNudgeStore` sample has no method that *sets* the
marker — a real gap: without it, AC 3 ("state row carries
`nudge_pending_at`") is unimplementable without touching `MessageStore`,
which AQ1 forbids (the storage contract stays closed to non-storage
callers). `mark_pending` is added to the trait, called from
`PreparedWrite::finish` after a newly-persisted deferred write. This write
is **post-commit and non-transactional** with the message insert: a crash
between the two leaves a durable, readable message with no pending marker —
durable-but-never-auto-nudged. This is the same failure class as today's
post-commit steer emission (`storage_and_nudge_router.rs:278-314`) and is
**accepted**, not a regression. `mark_pending` is conditional on `read = 0`
so a peer receipt/replay of an already-read message cannot re-mark it (see
"`newly_persisted` guard" below); a marker-write failure emits
`subsystem="atm_core.queue" action="queue_marker_set" outcome="failed"` and
must never fail the write itself — durable persistence always outranks the
best-effort marker.

### `GraftLeaseState` deviation (D7) and the AQ1.5 amendment

The sprint doc's classifier signature takes
`graft_lease: Option<&GraftReceiverLease>`. `GraftReceiverLease` does not
exist yet — it is an AQ1.5 type. Taking it as a parameter here would either
block AQ1 on AQ1.5's landing or require AQ1 to pre-declare a type it does not
own. Decision: `classify_delivery_channel` takes a 2-variant
`GraftLeaseState { Absent, Active }` owned by AQ1 instead. This avoids an
AQ1↔AQ1.5 type-name collision and lets AQ1 compile standalone; AQ1.5's plan
document needs a one-line amendment noting `GraftReceiverLease` is not the
classifier's input type — `GraftLeaseState` is, and AQ1.5/AQ2 wire the real
`GraftReceiverEndpointStore::lookup` result into it after AQ1.7 lands.
`GraftReceiverLease` as a direct classifier parameter is a **rejected
alternative** (see below), not a deferred one: it is superseded by
`GraftLeaseState`, not merely postponed.

### `MAX_NUDGE_ATTEMPTS = 5`

Fixed at `5` (`atm_storage::contract::MAX_NUDGE_ATTEMPTS`). Chosen as a
concrete, generous-but-bounded ceiling: enough auto-retries to absorb a
transient receiver-side outage across the recovery sweep's cadence (AQ3)
without an unbounded retry loop masking a genuinely stuck recipient. The
constant is the single source of truth for every recipient kind
(`claim_next_pending`'s eligibility predicate and (f)'s stuck-signal
threshold both read it); no sprint may define a second, per-channel retry
ceiling.

### Deferred rename inventory (§4) and the frozen-inventory gate

Per D8, the phase-ao2 terminology sweep is cut down to exactly two renames
in AQ1: `PostSendBuiltInTarget::LocalTmux` → `PostSendBuiltInTarget::LocalSteer`,
and the two new `NudgeKind`/`NudgeMode` enums. Everything below is
explicitly **deferred**, not silently dropped, and is enforced by
`scripts/check-nudge-taxonomy.py` (wired into `just lint` /
`.just/run_lint.py` as the `nudge-taxonomy` target): any *new*
`nudge`-family identifier introduced outside the script's frozen inventory
fails CI, so the deferred breadth below cannot grow by accident.

| Deferred item | Why | Owner |
| --- | --- | --- |
| Full phase-ao2 rename sweep (router/test-double, selector/emitter, `nudge_sink` family, kind-qualified log/event strings) | Breadth multiplies 3-way merge conflicts across the AQ1 shared worktree; AC 2 requires only that the changed set compiles with assertions updated | AQ2.6/AQ3 |
| `daemon_observability.rs:1084` dedupe | Legacy synchronous daemon; CLAUDE.md forbids patching it — struck from the sprint doc entirely | none (will not land) |
| Renaming `LocalTmuxNudgeTarget`'s payload | Payload is still tmux-shaped; AQ2.6 needs the eventual two-armed payload before a rename is meaningful | AQ2.6 |
| Migrating `runtime_health::MemberKey` onto the canonical `atm_storage::types::MemberKey` | Sprint doc says untouched; the type is private | non-blocking follow-up |
| `GraftReceiverLease` as a direct classifier parameter | Superseded by `GraftLeaseState` (D7) | n/a — rejected, see above |

### §3.2 invariant closure (`http-runtime.toml:41`)

Restated from (c): the AQ1 blueprint's integration-risk §3.2 flagged that a
deferred write's empty dispatch vector must remain legal under
`http-runtime.toml:41`'s unconditional post-write invariant. It is legal —
`send/mod.rs:397` already returns `Ok(Vec::new())` for the "no built-in
target" case, and `NudgeMode::Deferred` reuses exactly that path. No
manifest edit is required or made; this ADR is the record that the
invariant was checked and holds.

### §3.6a acceptance: blocking write in `finish`

The AQ1 blueprint's integration-risk §3.6a flagged that `mark_pending` is a
synchronous SQLite write inside `PreparedWrite::finish` (called from
`prepared.finish`, itself on the commit path). This is **accepted**: it is
the same blocking-write class as the existing durable ack-state transition
on that path, not a new architectural pattern. It does not touch the frozen
synchronous legacy daemon (CLAUDE.md's "never patch, harden, or remodel"
directive is inapplicable — `mark_pending` runs inside the `atm-core`
send/commit path, not the legacy daemon dispatch loop) and does not change
`atm-http-runtime`'s async admission profile: the SQLite write itself is the
existing single in-process write-worker seam
(`docs/adr/ADR-ATM-RUSQLITE-002.md`), not a new blocking point on the Tokio
runtime.

### `newly_persisted` guard on `mark_pending`

`mark_pending` must be gated on `PreparedWrite::is_newly_persisted()`
(`send/mod.rs:365-367`), mirroring the existing steer-emission guard. Without
this gate, a peer receipt or replay of an already-persisted message would
re-mark it pending, corrupting FIFO order and re-arming a message a
recipient may have already handled. This is a correctness requirement, not
an optimization; AQ1's test suite proves it directly (duplicate write → zero
`mark_pending` calls on the recording double).

## Consequences

- Every downstream sprint (Herdr AQ2.6/AQ2.7, graft AQ1.5–AQ1.9/AQ2, the
  delivery-trigger work AQ2.5, drain AQ3) implements `PendingNudgeStore`,
  `NudgeKind`/`NudgeMode`, `classify_delivery_channel`, and
  `rebuild_received_hook_dispatch` as given; none may add a variant, widen a
  signature, or define a competing type for the same concept.
- `atm queue` ships in AQ1 even though the tmux idle-drain trigger (AQ3)
  does not exist yet: a queued tmux message is durably readable and its
  full-surface parity with `atm send` is proved, but it is nudged only once
  AQ3's drain machinery lands. This is accepted scope, not a defect.
- The frozen-inventory gate (`scripts/check-nudge-taxonomy.py`) is a
  standing CI check, not a one-time migration script; it must be extended
  deliberately (new allowlist entries land beside the ADR/PR that
  introduces them), never bulk-regenerated to silence a finding.
- `ADR-018` §3's capability-trait cap is now at six traits including
  `PendingNudgeStore`; a seventh optional capability trait requires counting
  from this ADR forward, not from ADR-018's original baseline.

## Rejected alternatives

1. **Trait in `atm-core::boundary` (D1 option ii).** Rejected: forces
   `atm-storage-rusqlite -> atm-core`, a documented forbidden edge on two
   existing boundary manifests.
2. **`mark_pending` as a field on `Message`/`MessageStore`.** Rejected: the
   storage contract's closed list (ADR-018 §3) does not extend to
   non-storage callers reaching into `MessageStore`; `PendingNudgeStore` is
   the correct capability-trait seam.
3. **Writing the pending marker from the router.** Rejected: the router's
   production body above `storage_and_nudge_router.rs:696` is frozen by the
   `al3_*` architecture-test literals; a production-body edit there is a
   design regression per the AQ1 blueprint, not a valid implementation site.
4. **`GraftReceiverLease` as the classifier's direct parameter.** Rejected:
   the type does not exist until AQ1.5; blocking AQ1 on it, or having AQ1
   pre-declare a type it does not own, is worse than the narrow
   AQ1-owned `GraftLeaseState` with a documented one-line AQ1.5 amendment.
5. **Full phase-ao2 rename sweep landed in AQ1.** Rejected: breadth
   multiplies 3-way merge conflicts across this sprint's shared worktree
   with no functional benefit; the frozen-inventory gate makes the deferral
   safe rather than silent.
6. **Merging `NudgeKind` with Hermes's session-dispatch `mode`.** Rejected:
   conflating "which kind of nudge was this" with "how did the receiving
   session get told" collapses two independent axes into one enum and
   would force every future kind addition to also redefine Hermes's wire
   contract.

## Required evidence

- `cargo test -p atm-architecture` proves the `al3_*` literals are
  unchanged, the router production body above `:696` is untouched, and
  `PendingNudgeStore`/`MemberKey` compile in one crate with no new
  `atm-core` ↔ `atm-storage` cycle (`cargo tree` shows no new edge).
- `PendingNudgeStore` unit tests (mark→claim, concurrent claim race,
  requeue/max-attempts round-trip, release-without-attempt-increment,
  handoff clear vs. oldest-select, `list_pending_members` excludes
  read/deleted rows, FIFO across three ULIDs) and a read-path clear test.
- `send`-level tests proving `NudgeMode::Deferred` yields zero dispatches
  plus exactly one `mark_pending` call, `Immediate` is byte-identical to
  pre-AQ1 behavior, and a duplicate write calls `mark_pending` zero times.
- `scripts/check-nudge-taxonomy.py` (the `nudge-taxonomy` `just lint`
  target) passes once the `PostSendBuiltInTarget::LocalTmux` →
  `LocalSteer` rename lands; it enforces both the retired-identifier list
  and the frozen nudge-family inventory this ADR references.
- Boundary evidence: `boundaries/atm-storage/pending-nudge-store.toml` and
  `boundaries/atm-core/message-received-hook-emitter.toml` pass
  `.just/lint_boundaries.py`; `boundary-guard` review recorded before merge
  per (d).
- `atm queue <to> <msg>` full-surface parity truth-table against `atm send`
  (AC 3), including `--attach` parity arriving free from the shared
  `run_with_mode` implementation.

## AQ2.5 addendum — delivery-trigger policy

This addendum is normative for the AQ2.5 delivery-trigger implementation.
"Steer" and "queue" remain kinds, while the physical mechanism may defer a
steer-kind notification until a bare-CLI Stop pull; mechanism timing never
changes the kind.

| Kind | Classifier-owned channel | Delivery trigger | Mechanism |
| --- | --- | --- | --- |
| steer | Tmux, Herdr, or bare CLI | immediate post-persistence delivery, or next bare-CLI Stop pull | selected receiver hook or bounded RAM FIFO |
| queue | Graft, Herdr, or bare CLI | receiver queue handoff, sweep, or next bare-CLI Stop pull | published receiver, Herdr queue, or bounded RAM FIFO |

The `DeliveryChannel` classifier is the sole owner of channel selection; its
call sites and selector tests enforce this matrix. Channel names must describe
the positive mechanism (`QueuePull`, `Graft`, `Herdr`, or `Tmux`) rather than
an absence of another backend. Heartbeats are produced by harness hooks and
debounced there; the daemon records authenticated observations but owns no
timer or harness lifecycle.

Bare-CLI pull notifications are RAM-only daemon-lifetime state. The map is
bounded to 32 messages *per member*, drops the oldest item on overflow, and
reports the cumulative drop count through doctor. A daemon restart empties the
FIFO; durable mailbox messages remain authoritative. Each pull drains all
steer items and at most one oldest queue item. Empty pulls are successful and
must never block a Stop loop. Codex lifecycle integration uses the same
authenticated queue-get contract; a host-specific Codex drain adapter remains
an explicit coordination gap rather than an invented daemon-side fallback.

## Addendum (2026-08-27): Herdr retry partition (AQ2.7 ruling)

ADR-058 D8 outcomes are partitioned for ADR-054 (f) retry accounting: outcomes
where no input reached the agent (blocked, not-present family, infrastructure)
use `release_pending` (no attempt consumed) bounded by a per-member
consecutive-release cap (`HERDR_MAX_CONSECUTIVE_RELEASES = 10`, after which one
`requeue_pending` consumes an attempt); outcomes after input was injected or
ambiguous (`agent_prompt_stalled`, post-write errors/timeouts) use
`requeue_pending`. This keeps the (f) stuck signal reachable for Herdr
members. Normative text: sprint-AQ2-7 deliverable 3.

A marker-clear failure after a successful bare-CLI FIFO append (the handoff)
is never allowed to fail that delivery: it is routed through the same
`clear_queue_marker_after_handoff` retry-once-and-count helper AQ2's graft
channel uses (`atm-daemon-bootstrap/src/received_hook_selector.rs`), so the
failure is logged and counted while `emit_received_message` still returns
`Success` (AQ25-CRIT-001). Because bare-CLI members are never swept by AQ3
(only `TmuxSteer`/`Graft` members are), a marker that survives both clear
attempts becomes a **permanently orphaned pending marker** for that member —
there is no scheduler that will ever revisit it. This residual is disclosed,
not silently accepted as impossible: no sweeper covers `BareCli` by design,
and no one-shot clear-on-next-get retry is added either, per the sprint's
simplicity mandate (no additional hook-side or daemon-side state machines).
An operator can detect and clear the residual manually via the existing
`graft_queue_marker_clear_failures_total` doctor counter and
`PendingNudgeStore::clear_pending_on_read`, which any subsequent successful
delivery or `atm read` already exercises for that member.

### Accepted resilience tradeoffs (disclosed, not blocking)

- The high-frequency `Heartbeat` and `QueueGetNext` routes share the same
  single-permit `BlockingCoreBridge` bridge used by slower mailbox
  operations. Under sustained load this can add queueing latency to a
  heartbeat or queue-get call; it is not a correctness or fail-open risk
  (the bounded connect/request deadline still governs), and is accepted
  rather than given a dedicated admission lane in this sprint.
- The bare-CLI FIFO's 32-message-per-member bound is enforced per member;
  there is no additional cap on the aggregate memory used across all
  bare-CLI members on one daemon. A daemon serving an unusually large
  bare-CLI roster could accumulate proportionally more RAM. This is an
  accepted, documented tradeoff consistent with the simplicity mandate
  (RAM-only, restart-recoverable state); it is not a durability or
  correctness concern since the mailbox remains the source of truth.

### AQ2.5 quality-mgr sign-off

| Sprint | Gate | Reviewer | Date | Verdict | Notes |
| --- | --- | --- | --- | --- | --- |
| AQ2.5 | ADR-054 addendum (delivery-trigger policy, AC 9) | quality-mgr | _pending re-gate_ | _pending_ | Fill in on re-gate after the QA-1 fix cycle closes; mirrors AQ1 AC 1's ADR-054 sign-off gate. |

## Addendum (2026-08-27): Herdr retry partition (AQ2.7 ruling)

ADR-058 D8 outcomes are partitioned for ADR-054 (f) retry accounting: outcomes
where no input reached the agent (blocked, not-present family, infrastructure)
use `release_pending` (no attempt consumed) bounded by a per-member
consecutive-release cap (`HERDR_MAX_CONSECUTIVE_RELEASES = 10`, after which one
`requeue_pending` consumes an attempt); outcomes after input was injected or
ambiguous (`agent_prompt_stalled`, post-write errors/timeouts) use
`requeue_pending`. This keeps the (f) stuck signal reachable for Herdr
members. Normative text: sprint-AQ2-7 deliverable 3.

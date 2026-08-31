---
title: "Phase AV — Async mailbox-read cutover completion, hardening, and read benchmarks"
phase: AV
branch: plan/phase-av
sprint_branches: per-sprint, declared in each sprint doc's frontmatter;
  all five WILL form one `gh stack` rooted at integrate/phase-av once
  provisioning task AV.0 completes (AV.1a reuses the already-existing
  fix/mailbox-read-blocking-serialization, provisioned by team-lead and
  held clean off develop, to be adopted as the bottom of the stack).
  Current state: only fix/mailbox-read-blocking-serialization exists;
  integrate/phase-av and the four successor branches are not yet created.
status: hardening-in-progress
owner: fenix (plan author); arch-ctm (investigations I-1..I-5, implementation on approval)
base_revision: 938767c72 (develop)
integration_branch: integrate/phase-av
dependency_relations:
  - prerequisite: AV.1a
    dependent: AV.1b
    relation: must_follow
  - prerequisite: AV.1b
    dependent: AV.3
    relation: must_follow
  - prerequisite: AV.1b
    dependent: AV.4
    relation: must_follow
  - related: AV.2
    relation: parallel_safe
    scope: parallel_safe with AV.1a, AV.1b, AV.3, and AV.4 (docs-only footprint)
  - related: AV.3/AV.4
    relation: parallel_safe
    scope: gates vs. benchmark files, non-intersecting
---

# Phase AV — Async mailbox-read cutover completion, hardening, and read benchmarks

> Evidence base: arch-ctm's read-only investigation findings I-1..I-5
> (task INVESTIGATE-PHASE-AV-MAILBOX-READ, msg 01M1AJVGB9V5WXBGS2SF03KTS8,
> 2026-08-31), verified against develop @ 938767c72.

## 1. Problem — a cutover regression, not new architecture

`atm read --team atm-dev` intermittently fails after the same-host 3.25 s
client budget while the daemon completes the request 6–7 s later
(arch-ctm RCA, msg 01M1AJBANH1C43NECYF8VNKV68, reproduced locally).

Root cause: `StorageAndNudgeRouter` funnels **every** core job through
one `BlockingCoreBridge` holding **exactly one semaphore permit**
(definition `crates/atm-http-runtime/src/storage_and_nudge_router.rs:70-136`;
single-permit construction :171-181; `spawn_blocking` execution :83-121).
The bridge waits for the permit up to the request deadline
(`RequestDeadline`), then runs the job **non-cancellable** to completion —
elapsed time is observability, not enforcement. One slow job therefore
head-of-line blocks the entire read family past every other caller's
deadline; concrete interleaving: a deferred marker / doctor / clear starts
first, and an unrelated `read` waits behind it until its own 3 s request
budget expires with no DB contention of its own.

Complete bridge-client inventory (I-1), classified:

| Call site (`storage_and_nudge_router.rs`) | Operation | Class |
|---|---|---|
| :305-315 | deferred nudge marker (`PreparedWrite::mark_pending_if_deferred`) | post-write mutation/housekeeping |
| :493-511 | list (`list_mail_with_runtime`) | read |
| :514-533 | peek (`peek_mail_with_runtime`) | read |
| :536-555 | receive/read (`read_mail_with_runtime_impl`) | read **+ hidden state mutation** |
| :558-576 | clear (`clear_mail_with_runtime`) | mutation |
| :579-637 | doctor (`run_doctor_with_runtime[_ports]`) | control-plane read |
| :648-674 | heartbeat | roster-validation read |
| :677-700 | queue-get-next | roster read + in-memory FIFO drain |
| :703-811 | graft register/refresh/unregister/lookup | mutations + one read |

The hidden mutation in the read flow: `read_mail_with_runtime_impl` →
`resolve_read_display` (`crates/atm-core/src/read/mod.rs:188-209`) →
`apply_display_mutations_to_store` (:354-365) plus an optional
seen-watermark file write (:211-225). These belong on the ordered writer
lane after read-only selection.

### 1.1 Git archaeology — how the regression happened

- **AL3 (`bd7a45130`, 2026-08-06):** the single permit is born as
  `WriteAdmission::new(NonZeroUsize::new(1))`, expect-message
  **"one SQLite writer"** — a deliberate, correct bound *on the write
  path only*.
- **AL13-G7 (`1142c0ffe`, 2026-08-08):** `WriteAdmission` is renamed
  `BlockingCoreBridge` and split from a new `StorageWriterIngress`. The
  commit's own doc comment says the split exists to stop "read, doctor,
  and heartbeat bridging" from redefining "the storage writer's batching
  capacity" — yet the read-side bridge was instantiated with the same
  capacity 1, and the expect-message was reworded to "one non-storage
  core bridge operation". The writer's bound was rationalized into a
  read-path bound instead of being replaced with a read concurrency
  model.
- The legacy sync daemon was thread-per-request: each request ran its
  sync core call on its own thread with its own storage handle, so reads
  **were** naturally concurrent WAL readers. The Tokio cutover kept the
  sync calls, bridged them, and made the replacement *less* concurrent on
  reads than the daemon it replaced. Phase AV completes the cutover the
  AL phase left unfinished.

Second serialization layer: the nominally-async read path is a write
queue in disguise — `WriteOp::ListMessages`
(`crates/atm-storage-rusqlite/src/writer/ops.rs:37,106`) is submitted to
the single SQLite writer thread by `SharedDb::submit_list_messages_async`
(`shared_db.rs:482-501`), which `AsyncMessageStore::list_messages_async`
delegates to (`lib.rs:612-615`). Reads queue behind writes by
construction. Separately, the existing `SearchReader` is a single worker
(`search_reader.rs:40-75`) — its bounded mpsc/oneshot/deadline shape is
the right pattern, but one worker cannot serve mailbox fan-out.

### 1.2 Design intent this violates (Rand, 2026-08-30)

The message schema was specifically designed with an **immutable primary
message** and **race-tolerant state**: a read racing a state change may
return either value ("don't care"). Therefore reads require **zero**
coordination with the writer lane — no read-your-writes, no freshness
guarantee, no snapshot pinning, no fencing. SQLite WAL natively supports
N concurrent readers beside one writer; the runtime serializes what both
the schema contract and the storage engine explicitly permit. The reader
pool is pure resource management (bound + deadline), never consistency
machinery. Any design or QA argument that mailbox reads must be ordered
through or fenced against the writer contradicts stated schema intent.

## 2. Acceptance contract (Rand, binding for the phase)

1. Axum handlers use async mailbox APIs end-to-end; no
   `spawn_blocking(read/list/peek/...)` and no mailbox read enters a
   blocking bridge.
2. Multiple `read`, `peek`, `list`, and `doctor` requests are serviced
   concurrently across large team/agent fan-out; a slow operation for
   one mailbox/team cannot delay another.
3. A bounded multi-reader async mailbox-query capability: separate
   reader connections/lanes with an explicit concurrency bound and
   deadline/backpressure — not a single writer queue hidden behind an
   async signature (the existing search reader's single thread is also
   insufficient for fan-out).
4. Only narrowly scoped state mutations ride the ordered async writer
   lane, after parallel selection/query work.
5. Doctor is independently async/schedulable; it never occupies the
   mailbox-query capacity.
6. Regression proof: many concurrent cross-team read/peek/list/doctor
   calls while a deliberately stalled housekeeping operation and
   unrelated writer activity run — all independent reads complete within
   budget; bounded overload fails explicitly rather than serializing
   indefinitely.

Additional phase mandates (Rand, 2026-08-30/31):

7. **Hardening:** architecture/requirements amended so the current mode
   of operation is *mechanically non-compliant* — hard gates, not review
   vigilance (§ AV.2/AV.3).
8. **Read + query benchmarks:** benchmark targets proving reads and
   queries execute in a massively parallel manner, with ratcheted floors
   (§ AV.4).

## 3. Sprints

Sprint docs are the authoritative source for deliverables, acceptance
criteria, and required validation. This section is a map only.

| Sprint | Doc | Scope |
|---|---|---|
| AV.1a | [sprint-AV.1a-reader-lane-foundation.md](./sprint-AV.1a-reader-lane-foundation.md) | Bounded RO WAL reader pool (defaults, per-job transaction scoping, startup connection budget), sealed scope-enforcing async mailbox-read capability with storage-owned `ReadDeadline` and boundary TOML records, `SearchReader` re-hosted as a second pool instance, `AsyncMailboxRuntime` port + pure-selection extraction, cancellable read deadlines with capacity reclamation, per-lane metrics seams. Runtime-inert: no handler consumes it yet. |
| AV.1b | [sprint-AV.1b-read-handler-cutover.md](./sprint-AV.1b-read-handler-cutover.md) | The atomic behavior change: read-family handler cutover through the async port, hidden-mutation split under a single admission-based handoff protocol, typed doctor decomposition with per-leg lanes/deadlines, writer purity, liveness tests. Atomicity rationale recorded in the sprint doc. |
| AV.2 | [sprint-AV.2-requirements-adr-hardening.md](./sprint-AV.2-requirements-adr-hardening.md) | Normative MUST rules for read concurrency and race-tolerant state; reader/writer-lane ADR with the AL13-G7 regression as history; phase-AV closeout record (the Phase-AM ledger is frozen and is not edited). |
| AV.3 | [sprint-AV.3-mechanical-hard-gates.md](./sprint-AV.3-mechanical-hard-gates.md) | `BlockingCoreBridge` identifier deleted and residual bridge renamed/narrowed to an exact enumerated control-path call-site set (residual migration = `AV-FU-1`), read-family architecture guard incl. the `atm-runtime` composition layer, WriteOp purity lint, liveness tests owned as permanent CI gates. |
| AV.4 | [sprint-AV.4-read-query-benchmarks.md](./sprint-AV.4-read-query-benchmarks.md) | Massively parallel read/peek/list and query/search benchmark families, mixed read-under-write-load mode, ratcheted per-host floors, reader-lane diagnostics in reports — additive family registration only, under the recorded benchmark-infra freeze exception (mandate 8). |

Dependency relations (rationale in each sprint doc's frontmatter):
AV.1a→AV.1b, AV.1b→AV.3, and AV.1b→AV.4 are `must_follow` (cutover
consumes the foundation; gates and benchmarks assert the post-cutover
state); AV.2 is `parallel_safe` with all others; AV.3∥AV.4 are
`parallel_safe`. Once the stack is provisioned (task AV.0, §4),
propagation of parent changes happens by restacking the `gh stack`,
which replaces manual merge-forward.

## 4. Execution notes

- All daemon work targets the Tokio+Axum `atm-http-runtime` path only;
  the frozen legacy sync daemon is untouched (AGENTS.md hard rule).
- Branch strategy — stacked branches (`gh stack`), **planned, not yet
  provisioned**: all five sprint branches will form one stack rooted at
  `integrate/phase-av`, in stack order AV.1a → AV.1b → AV.2 → AV.3 →
  AV.4. Provisioning is task **AV.0** (owner: team-lead; pre-implementation
  gate for every sprint): create `integrate/phase-av` from `develop`,
  create the four successor branches, run the commands below, and record
  the non-interactive `gh stack view --json` output in this section as
  completion proof. No sprint may be dispatched, and restack may not be
  cited as a propagation mechanism, until that proof is recorded here:

  ```sh
  gh stack init --base integrate/phase-av \
    fix/mailbox-read-blocking-serialization \
    feature/av1b-read-handler-cutover \
    docs/av2-read-concurrency-requirements \
    feature/av3-read-concurrency-gates \
    feature/av4-read-query-benchmarks
  gh stack submit --auto   # bottom PR targets integrate/phase-av; each other PR targets its parent
  ```

  Operate the stack via the `/gh-stack` skill — installed for Claude
  agents at `~/.claude/skills/gh-stack/SKILL.md` and for Codex agents at
  `~/.codex/skills/gh-stack/SKILL.md` (same content) — non-interactive
  rules apply (positional branch args, `submit --auto`, `view --json`,
  `rerere.enabled`). A successor skill is being developed on
  synaptic-canvas; adopt it when it lands.

  AV.1a will adopt the existing held branch
  `fix/mailbox-read-blocking-serialization`. Stack order is the
  dependency chain; AV.2's mid-stack position is ordering convenience
  only (it is `parallel_safe` with everything and carries no code).
  Once AV.0 is complete, parent changes propagate by restacking instead
  of manual merge-forward; merges remain merge-commit only (never
  squash).

  AV.0 completion proof (`gh stack view --json`, team-lead, 2026-08-31,
  msg 01M1AQN768YNDBZPHJYJ53EDDY): trunk `integrate/phase-av` created
  off `develop@db08f4591` (post PR #1108 merge) and pushed; stack
  `integrate/phase-av ← fix/mailbox-read-blocking-serialization ←
  feature/av1b-read-handler-cutover ← docs/av2-read-concurrency-requirements
  ← feature/av3-read-concurrency-gates ← feature/av4-read-query-benchmarks`,
  all five branches on origin at `db08f4591` (empty layers), none
  merged/queued/needing rebase. PRs open automatically via
  `gh stack submit --auto` once a branch receives its first commit
  (GitHub refuses a zero-diff PR). AV.1a (arch-ctm) and AV.2
  (Cipher-311d) dispatched 2026-08-31 after Rand merged the plan.

  AV.0 completion proof (`gh stack view --json`, captured 2026-08-31):

  ```json
  {"trunk":"integrate/phase-av","branches":[
    {"name":"fix/mailbox-read-blocking-serialization","pr":1112},
    {"name":"feature/av1b-read-handler-cutover","pr":1115},
    {"name":"docs/av2-read-concurrency-requirements","pr":1110},
    {"name":"feature/av3-read-concurrency-gates","pr":1113},
    {"name":"feature/av4-read-query-benchmarks","pr":1114}
  ]}
  ```
- Adjacent-work sequencing: the #1030 WPERF plan touches the same writer
  path — coordinate worktrees/merge order with team-lead.
- Benchmark-infra freeze: a standing directive freezes new
  benchmark-infrastructure work. AV.4 is the one recorded exception,
  authorized by mandate 8 (§2) and bounded to additive family
  registration on the existing send-message-benchmark machinery (AV.4
  D6/A6). Any AV.4 work beyond that bound needs Rand's sign-off first.
- Explicit residual — follow-up **AV-FU-1** (out of AV scope, not
  hidden): after AV.3, `ControlPathSyncBridge` (renamed from
  `BlockingCoreBridge`) remains with exactly **eight** non-read call
  sites in `crates/atm-http-runtime/src/storage_and_nudge_router.rs`
  (12 at HEAD, 4 migrated by AV.1b): the deferred-queue marker inside
  `commit_write` (`retry_deferred_marker`, intentionally synchronous by
  contract), `clear_messages` (mutation via synchronous
  `clear_mail_with_runtime`; no writer-ingress op exists yet),
  `heartbeat`, `queue_get_next` (synchronous roster check
  `validate_heartbeat_member` + in-memory work), and
  `graft_receiver_register` / `_refresh` / `_unregister` / `_lookup`
  (roster check + synchronous `GraftReceiverEndpointStore`). None is a
  mailbox read; none is on the acceptance contract. Deleting the bridge
  for real requires an async roster/member-validation port, an async
  graft-receiver-store port, and a `WriteOp::ClearMailbox` ingress op —
  a separate, later plan. AV.3 A1 pins the call-site set so it cannot
  grow silently in the meantime.
- Plan QA: quality-mgr review before any implementation dispatch.

## 5. QA history

| Round | Date | Reviewer | Result | Notes |
|---|---|---|---|---|
| — | 2026-08-31 | — | pending | I-1..I-5 evidence incorporated (msg 01M1AJVGB9V5WXBGS2SF03KTS8); awaiting quality-mgr round 1. |
| crit-1..3 | 2026-08-31 | arch-ctm (critical-plan-reviewer) | cap-exhausted / not converged | 3 cycles (reviewed 49945f609 → b213f279e → 7af2b2ecf). Cycle 3 residual: CRIT-014 (explicit mailbox/doctor port split), CRIT-015 (supervisor lifecycle/fault contract), M1 (metric outcomes) — all corrected at 39f3d23ed, unverified by arch-ctm (no cycle 4 permitted). |
| qm-r1 | 2026-08-31 | quality-mgr (req-qa, arch-qa, ruthless-boundary-qa) @ 7af2b2ecf | FAIL 3B/5I/1m | B1 bridge deletion unreachable → AV.3 D1 rename+enumerate, `AV-FU-1`; B2 frozen AM ledger → AV.2 D4 phase-AV closeout record; B3 `RequestDeadline` in atm-storage → storage-owned `ReadDeadline`; I1 pool defaults/knob location/connection budget (AV.1a D2); I2 per-job transaction scoping + WAL-health test (AV.1a D2/A3d); I3 freeze exception recorded (AV.4 D6/A6, §4); I4 boundary TOML records (AV.1a D1a); I5 `SearchReader` re-hosted as second pool instance (AV.1a D2a, AV.4 D2); M1 composition-layer gate sibling (AV.3 D2/D5). Report: PR #1108 comment. |
| qm-r2 | 2026-08-31 | quality-mgr @ 399527808 | FAIL (narrow) 2B | 7/9 r1 verified fixed. AV-R2-B1: residual enumeration omitted `clear_messages` (:566) → 8 residual sites everywhere (AV.3 D1/A1/RESIDUAL, §4 AV-FU-1, AV.1b out-of-scope). AV-R2-B2: connection-budget arithmetic (16 vs 22) → per-lane `max_quarantined`, worst case 22 / steady 12 under 32 (AV.1a D2, contract, A3e). Report: PR #1108 comment. |
| qm-r3 | 2026-08-31 | quality-mgr @ c76f5ab85 | **PASS** | Scoped fact-trace: AV-R2-B1 and AV-R2-B2 fixed; 10/10 deliverable completion; plan QA gate PASS (msg 01M1AQAK7QDG66S6QEC4YQT4BN). Implementation dispatch still gated on AV.0 provisioning proof (§4) and Rand's approval. |

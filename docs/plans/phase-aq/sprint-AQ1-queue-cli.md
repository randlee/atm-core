# Sprint AQ1 — Trait Foundation + `atm queue`: CLI Verb, Taxonomy, and Storage Contract

Status: draft · Branch: `feature/aq-1-queue-cli` off `integrate/phase-aq`
(created from `develop` at phase start; mechanical precondition on the cut
head: `test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`) ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

**Re-scoped 2026-08-26 (per Rand, critical review B2/B5/B6/B7): this is the
trait-change sprint.** It lands every contract the Herdr (AQ2.6/AQ2.7),
graft (AQ1.5–AQ1.9, AQ2), delivery-trigger (AQ2.5) and drain (AQ3) sprints
build on — see "Trait-foundation scope" below — so that later sprints are
implementers, never definers. Adds `atm queue` — `atm send` with the nudge
deferred until the recipient harness is ready — together with the taxonomy
and code refactor it rests on.
**Nudge** is the umbrella term for post-delivery recipient notification;
**steer** (immediate — today's only kind) and **queue** (deferred) are its
two kinds, used consistently through daemon code.

Verified baseline (integrate/phase-ao2): the steer nudge fires synchronously
post-persistence — `emit_received_hook` call site at
`storage_and_nudge_router.rs:538` (definition :234), guarded only by
`if committed.newly_persisted`, single-call-site pinned by the `al3_*`
architecture test in `boundary_enforcement.rs` (literal string assertions);
no deferral surface exists. `mail_message_states` migrates via
`ensure_column` (`shared_db.rs:880`). Real hook-family names:
`TokioTmuxReceivedHook`, `PublishedGraftReceivedHook`,
`ReplacementReceivedHookSelector`, `MessageReceivedHook{Emitter,Selector}`,
`PreparedWrite::build_received_hook_dispatches`
(`atm-core/src/send/mod.rs:391-417`).

## Deliverables

1. **ADR-054 nudge-taxonomy-and-queue-mechanism**, quality-mgr-reviewed,
   deciding with rationale:
   (a) the taxonomy (aligned with, and disambiguated from, Hermes's
   session-dispatch `mode="queue"|"steer"`);
   (b) `nudge_pending_at` column + derived-FIFO + atomic-claim semantics;
   (c) the steer-suppression seam — caller-owned in
   `PreparedWrite::build_received_hook_dispatches` per ADR-019; the router
   call site, its `newly_persisted` guard, the `al3_*` test, and
   `http-runtime.toml`'s unconditional post-write invariant stay untouched;
   (d) `PendingNudgeStore` governance via the ADR-018 §3 follow-up process
   (amendment to ADR-036, `boundaries/atm-storage/pending-nudge-store.toml`,
   `atm-architecture` test, `boundary-guard` review as merge precondition —
   the governance chain runs **in parallel** with this sprint's dev and
   test work and gates only the final merge, never intermediate review:
   the ADR-018 §3 amendment and TOML record are authored alongside
   deliverable 6, and dev/QA on deliverables 2–5 proceed independently, so
   a multi-round boundary-guard review delays merge without blocking or
   invalidating tested work);
   (e) `MemberStateTransitionSink`'s relationship to ADR-019 and
   `RuntimeHealth`'s observability scope (implemented AQ3);
   (f) the graft dual-channel contract + handoff failure policy, including
   **bounded re-attempts**: a concrete max auto-retry count for failed
   handoffs (recovery-sweep re-attempts, AQ3), after which the marker stays
   set but auto-retry stops and a distinct "stuck" health signal surfaces
   for operator action (implemented AQ2/AQ3);
   (g) rename/compat policy: `.atm.toml` `post_send_hooks` key and the
   external command-hook system are a DISTINCT mechanism — NOT renamed;
   `NudgeTemplateOverrideStore` cluster keeps its names (already
   umbrella-sense); wire-crossing contracts (`GraftPostSendRequest`/
   `Response` loopback TCP — receiver process can lag/lead the daemon —
   and the `ATM_INTERNAL_NUDGE`/`InternalNudgeEnvelope` env payload)
   change only with an explicit both-sides plan; `PyNudge` and the Python
   callback shape kept (future rename via deprecation shim); `atm doctor
   --json` field names kept.
2. **Kind-aware dispatch + rename** (one change set, updating the
   `boundary_enforcement.rs` literal assertions in the same commit):
   `BuiltInPostSendDispatch` (or successor) carries `NudgeKind::Steer |
   Queue`; `ReplacementReceivedHookSelector` routes both — in AQ1 the
   `Queue` arm resolves to no emitter (`None`); AQ2/AQ3 own the
   queue-kind emitters that consume it; `build_built_in_dispatch`
   (`send/hook.rs:17`) gains the kind decision. `NudgeKind`
   (`atm-core::boundary`) records what was dispatched; `NudgeMode`
   (`atm-core::send`, caller-owned per ADR-019) records what the caller
   requested; the mapping is fixed and one-directional:
   `Immediate -> Steer`, `Deferred -> Queue` (M1). Rename scope is cut to
   `PostSendBuiltInTarget::LocalTmux -> LocalSteer` plus the new
   `NudgeKind`/`NudgeMode` types — breadth multiplies 3-way lane conflicts
   in the shared AQ1 worktree, so the rest of the phase-ao2 sweep is
   deferred (see "Deferred renames" below). A terminology grep-gate
   (precedent: `scripts/check-legacy-mailbox-paths.py`) enumerated in CI
   fails new "nudge"-named identifiers where a kind is meant, so the
   deferral cannot create new ambiguity.

   **Deferred renames (frozen inventory, gated by
   `scripts/check-nudge-taxonomy.py`):** out of AQ1's rename scope, picked
   up as later sprints touch the relevant files — atm-core boundary family
   (beyond `NudgeKind`), send family (beyond `NudgeMode`), atm-http-runtime
   router/test doubles, atm-daemon-bootstrap selectors/emitters, atm-graft
   `nudge_sink` family, remaining kind-qualified log/event strings.
   Recorded in the ADR-054 appendix.

   Struck from AQ1's scope entirely (not deferred): the
   `daemon_observability.rs:1084` dedupe item. Reason: legacy daemon —
   CLAUDE.md forbids patching; Phase AM deletion target.
3. **`atm queue` CLI verb**: clap surface mirrors `atm send` exactly
   (shared implementation, `NudgeMode::Immediate | Deferred` as the only
   fork — `NudgeMode` lives in `atm-core::send` alongside `WriteRequest`,
   caller-owned per ADR-019). Same cancel semantics as send. (`--attach`
   parity arrives automatically when AQ4 adds it to the shared surface.)
4. **`nudge_pending_at` + `nudge_attempts` columns** on
   `mail_message_states` via `ensure_column`. Set at write time for queued
   messages; FIFO derived (unread + pending, ULID order — restart-safe, no
   in-memory truth). `nudge_attempts` is the single owner of retry state
   for every recipient kind — no sprint keeps its own attempt tracking.
5. **Steer-suppression + read-path clear**: a `Deferred` write omits the
   steer dispatch inside `build_received_hook_dispatches` (which already
   returns `Ok(Vec::new())` for no-dispatch); the read-state transition
   that sets `read = 1` also clears `nudge_pending_at` in the same update —
   implemented in the whole-row upsert statement at
   `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs:28-36` via
   `nudge_pending_at = CASE WHEN excluded.read = 1 THEN NULL ELSE
   mail_message_states.nudge_pending_at END` (no new bind; one statement
   covers read/peek-mutating/ack/clear because all funnel through the same
   upsert). Suppression and any queue-kind dispatch each emit structured
   events with `subsystem`/`action`/`outcome`.
6. **`PendingNudgeStore`** (owned by `atm-storage`; fixed/internal, ADR-001
   sealed-supertrait pattern; sync methods — async callers use
   `spawn_blocking`):

```rust
/// THE canonical public member key for nudge/queue surfaces, defined in
/// `atm-storage::types` (re-exported by `atm-core::boundary`) — the
/// backend that must implement `PendingNudgeStore` (`atm-storage-rusqlite`)
/// cannot see atm-core, so the key cannot live there (crate-placement
/// decision D1 below). Distinct from the PRIVATE
/// runtime_health::MemberKey (runtime_health.rs:43), which is
/// intentionally untouched here; a non-blocking follow-up may migrate it
/// onto this type. AQ2.5's BareCliMemberKey has the identical shape and
/// is superseded by this type — AQ2.5 uses THIS key (see its deliverable
/// 3 note and the derivation at PullPendingReceivedHook::emit).
pub struct MemberKey { pub team: TeamName, pub agent: AgentName }

pub enum NudgeMode { Immediate, Deferred }

/// Maximum automatic delivery attempts for one deferred (queue-kind) nudge.
/// At or above this count the marker stays set but becomes auto-retry
/// ineligible and the row is reported stuck (ADR-054 (f)).
pub const MAX_NUDGE_ATTEMPTS: u32 = 5;

pub trait PendingNudgeStore: sealed::Sealed + Send + Sync {
    /// Marks one just-admitted message as awaiting a deferred nudge.
    /// Conditional on the row still being unread (`read = 0`) and not
    /// deleted; returns whether the marker was set. Called post-commit,
    /// non-transactionally with the message insert (`PreparedWrite::finish`)
    /// — a crash in that window yields durable-but-never-nudged, the same
    /// class as today's post-commit steer emission. Marker-write failure is
    /// logged and must never fail the write.
    fn mark_pending(&self, member: &MemberKey, msg: &AtmMessageId, at: IsoTimestamp)
        -> Result<bool, AtmError>;
    /// Atomically select-and-claim the oldest eligible pending message
    /// (unread, marker set, attempts < MAX_NUDGE_ATTEMPTS) in ULID order and
    /// clear its marker, returning the attempt number. `None` = nothing
    /// eligible, or another caller won the race. THE at-most-once
    /// mechanism: one conditional UPDATE ... RETURNING, shared verbatim by
    /// the idle-transition drain and the recovery sweep.
    fn claim_next_pending(&self, member: &MemberKey)
        -> Result<Option<NudgeClaim>, AtmError>;
    /// Dispatch of a claimed nudge failed: re-set the marker with
    /// attempts = attempt + 1. At/over MAX_NUDGE_ATTEMPTS, the marker stays
    /// set but becomes ineligible for auto-retry and is flagged stuck (the
    /// health signal in ADR-054 (f)).
    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim)
        -> Result<(), AtmError>;
    /// Restores a claim when the selected delivery mechanism refused to
    /// attempt input for a lifecycle reason (AQ2.7's `agent_blocked`
    /// result). Restores the marker for exactly this claimed message without
    /// incrementing `nudge_attempts`: no input was injected, so this is not a
    /// delivery retry. Conditional/idempotent on the claim identity.
    fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim)
        -> Result<(), AtmError>;
    /// Read-path clear (same state update that sets read = 1).
    fn clear_pending_on_read(&self, member: &MemberKey, msg: &AtmMessageId)
        -> Result<(), AtmError>;
    /// Synchronous-handoff clear for ONE named message: the caller has
    /// just handed exactly this message to its channel (AQ2 graft
    /// queue-kind wire handoff; AQ2.5 bare-CLI FIFO append) and clears
    /// exactly its marker — unconditional, idempotent (clearing an
    /// already-clear marker is Ok). Distinct from `claim_next_pending`
    /// (oldest-select for drain/sweep) and `clear_pending_on_read`
    /// (read path); with a backlog present, the just-handed-off message
    /// is NOT necessarily the oldest, so oldest-select must never be
    /// used for handoff clears.
    fn clear_pending_on_handoff(&self, member: &MemberKey, msg: &AtmMessageId)
        -> Result<(), AtmError>;
    /// Enumerate members that currently hold at least one eligible pending
    /// marker (unread, marker set). Consumed by AQ3's recovery sweep and
    /// AQ2.7's Herdr pump, which otherwise have no way to discover WHICH
    /// members to claim for (critical review B7). Order unspecified; callers
    /// apply their own channel pre-check before claiming.
    fn list_pending_members(&self)
        -> Result<Vec<MemberKey>, AtmError>;
}

pub struct NudgeClaim { pub msg: AtmMessageId, pub attempt: u32 }
```

No raw SQL above the backend crate (`no_backend_specific_message_contract`
gate; `message-store.toml`'s closed contract list unaffected).

## Trait-foundation scope (added 2026-08-26)

These contracts are AQ1 deliverables; no later sprint may define or widen
them (they may only implement them):

- **Crate placement of `PendingNudgeStore` + `MemberKey` — decided: option
  (i), `atm-storage` (B2).** `atm-storage-rusqlite` — the crate that must
  implement `PendingNudgeStore` — depends on `atm-storage` only, not
  `atm-core` (`atm-storage-rusqlite/Cargo.toml:18-19`); a trait in
  `atm-core::boundary` would force the forbidden `atm-storage-rusqlite ->
  atm-core` edge (option (ii), rejected). `MemberKey` therefore lives in
  `atm-storage::types` beside `TeamName`/`AgentName`, and the trait lives in
  `atm-storage::contract`; `atm-core::boundary` re-exports both. The sample
  above carries the real error type (`AtmError` — `StorageError` does not
  exist) and the `sealed::Sealed + Send + Sync` supertrait bound.
- **`mark_pending`** (above) — the real gap: nothing in the earlier trait
  sketch *sets* the marker, so AC 3 ("state row carries `nudge_pending_at`")
  was unimplementable without touching the closed `MessageStore` contract.
  Selected call site: `PreparedWrite::finish`, post-commit and
  non-transactional with the message insert — record the crash-window
  disclosure (durable-but-never-nudged, same class as today's post-commit
  steer emission) in ADR-054. Conditional on `read = 0`.
- **`list_pending_members`** (above; B7).
- **Dispatch-from-message-id (B6).** A `BuiltInPostSendDispatch` today is
  built only from in-memory `PreparedWrite` planning data
  (`send/hook.rs:17`, `send/mod.rs:391-417`, "never reloads the committed
  record"). AQ3's drain/sweep and AQ2.7's pump re-dispatch a persisted
  `NudgeClaim { msg, attempt }`. This sprint defines
  `atm_core::nudge_dispatch::rebuild_received_hook_dispatch(runtime:
  &LocalServiceRuntime, member: &MemberKey, message_id: AtmMessageId, kind:
  NudgeKind) -> Result<Option<BuiltInPostSendDispatch>, AtmError>` in a new
  module outside `send/` (the architecture test asserts the planner never
  reloads from `send/`); drain/sweep/pump call it and never re-implement it.
- **`LocalMessageReceivedBackend` + `DeliveryChannel` classifier seam,
  owned once (B5).** The roster-owned
  `enum LocalMessageReceivedBackend { Tmux { pane_id: PaneId }, Herdr { session: Option<HerdrSession> } }`
  where **`HerdrSession` is defined HERE** (`crates/atm-core/src/delivery_channel.rs`):
  `pub struct HerdrSession(String)` — validated newtype (non-empty, no
  whitespace/control characters), `new(impl Into<String>) -> Result<Self, AtmError>`,
  `as_str()`, `Display`, derives `Debug/Clone/PartialEq/Eq/Hash`,
  `#[serde(transparent)]`; derived from `metadata_json["herdrSession"]`
  (invalid/empty → `None` with a `tracing::warn`). AQ2.6 and ADR-058
  consume this type; neither defines it.
  (`session` sets `HERDR_SESSION` on the Herdr emitter's child process
  environment per invocation, `None` = Herdr's default server — the daemon
  never launches Herdr sessions, so which session a member's agent lives in
  is roster data set by the external launcher, exactly like tmux
  `pane_id`) and the pure
  `classify_delivery_channel(local_backend: Option<&LocalMessageReceivedBackend>,
  graft_lease: GraftLeaseState) -> DeliveryChannel { TmuxSteer, HerdrSteer,
  Graft, BareCli }`, with `enum GraftLeaseState { Absent, Active }`
  replacing the earlier `Option<&GraftReceiverLease>` parameter to avoid an
  AQ1↔AQ1.5 type-name collision (D7), are defined HERE, together with the
  `PostSendBuiltInTarget::LocalSteer` rename and the backend→channel
  mapping in exactly one function. A
  `local_message_received_backend(member: &RosterEntry) ->
  Option<LocalMessageReceivedBackend>` derivation is also defined HERE:
  `recipient_pane_id` → `Tmux`; `metadata_json["backendType"] == "herdr"` →
  `Herdr { session: metadata_json["herdrSession"] }`; else `None` — no
  schema migration, both fields are already persisted roster columns. AQ2.6
  implements the Herdr emitter + CLI/doctor surface for the enum; AQ2.5
  implements the `BareCli` arm's FIFO/pull; neither adds variants. AQ1.7
  maps `GraftReceiverEndpointStore::lookup` to `GraftLeaseState` once AQ1.5
  lands (see sprint-AQ1-5's amendment).
- **Sealed `AsyncMessageReceivedHookEmitter` extension point (I13).** The
  boundary manifest `boundaries/atm-core/message-received-hook-emitter.toml`
  is brought current (it names `GraftReceiveHook`, a sync impl, and omits
  `PublishedGraftReceivedHook`) and its implementer list becomes the
  authoritative count later sprints' manifest ACs (AQ2.5 AC 11, AQ2.6)
  extend.

Invariants: a queued message is readable immediately; reading clears its
marker; markers survive daemon restart; a message is deferred-nudged at
most once.

## Acceptance criteria

1. ADR-054 merged, decisions (a)–(g) closed, quality-mgr sign-off recorded.
2. Rename change set compiles green on all three CI lanes with the
   `boundary_enforcement.rs` assertions updated in the same commit; compat
   surfaces proven unchanged (`[[atm.post_send_hooks]]` fixtures parse;
   doctor JSON fields unchanged; `GraftPostSendRequest` wire JSON
   unchanged; `PyNudge` attributes unchanged — hermes-atm tests green).
3. `atm queue <to> <msg>` delivers a durably readable message immediately
   with no steer-kind emission (no tmux send-keys, no graft steer channel);
   state row carries `nudge_pending_at`. Full-surface parity truth-table
   with `atm send`. `mark_pending` is gated on `is_newly_persisted()`: a
   duplicate or peer-replay write never re-marks an already-persisted row.
4. Reading a queued message before any nudge clears its marker; daemon
   restart with pending rows re-derives the FIFO (query test).
4a. `claim_next_pending`/`requeue_pending` round-trip: a failed dispatch
   requeues with an incremented attempt; at the ADR-054 (f) max the row
   becomes auto-retry-ineligible and flags stuck (no unbounded retry).
   `release_pending` restores only the same claim without changing its
   attempt count; AQ2.7 proves an `agent_blocked` Herdr rejection uses this
   release path rather than consuming retry budget.
5. Kind-aware dispatch test: Steer dispatches route through the selector to
   the tmux emitter; Queue dispatches route to no emitter (`None`) — AQ2/AQ3
   own the queue-kind emitters that consume them.
6. Boundary governance: ADR-018 §3 amendment + pending-nudge-store TOML +
   `cargo test -p atm-architecture` green + `boundary-guard` review.
7. Terminology grep-gate enumerated in CI. `just test` all three lanes.
8. Trait-foundation: `PendingNudgeStore`/`MemberKey` compile in
   `atm-storage` with the `sealed::Sealed + Send + Sync` bound enforced and
   no atm-core↔atm-storage cycle (`cargo tree` shows no new edge);
   `mark_pending`/`list_pending_members` round-trip against seeded rows;
   `atm_core::nudge_dispatch::rebuild_received_hook_dispatch` rebuilds a
   dispatch equal to the write-time one for a fixture message;
   `classify_delivery_channel(Option<&LocalMessageReceivedBackend>,
   GraftLeaseState) -> DeliveryChannel` is table-tested over all four
   channels (`HerdrSteer` via the enum variant, no emitter required), plus
   `local_message_received_backend` derivation cases; emitter boundary
   manifest matches the real implementer set.

## Paths to delete

None beyond identifier renames; no behavior change to `atm send`.

## Required validation

- Mechanical dispatch gate on the cut head (ADR-047/053 presence, above).
- `just test` + `cargo test -p atm-architecture`, ubuntu/macOS/Windows;
  hermes-atm Python tests green.

## Non-closure / out of scope

- Graft queue-channel delivery (AQ2). Tmux idle-drain (AQ3) — until then a
  queued tmux message is nudged only via the AQ3 machinery (not yet built);
  the verb still ships because the message is durably readable.
- Repo-doc terminology updates: landed on the plan branch during planning.
- Marker write placement (revised 2026-08-26, quality-mgr QM40-B1 — the
  earlier "accepted blocking" note is withdrawn): `mark_pending` for a
  Deferred write runs on the router's blocking lane (inside the same
  `spawn_blocking` hop as the canonical state upsert, or an adjacent one
  gated on `newly_persisted && Deferred`), never directly on a Tokio worker
  from async `commit_write`; a Deferred-write test through the real async
  router + SQLite is required (AC 3).

## Dependencies

- must_follow: none — AQ1 is the phase root and the trait-change sprint.
- parallel_safe: none (every later sprint implements contracts defined
  here). Plan-doc lanes A/B/C/D (see plan "Parallel lanes") may run
  alongside because they touch none of this sprint's files.
- Downstream: AQ2.6/AQ2.7 are the first implementers (Herdr, most urgent
  per Rand 2026-08-26); AQ1.5–AQ1.9 run parallel_safe with them (disjoint
  files).

# Sprint AQ1 — `atm queue`: CLI Verb, Taxonomy, and Storage Contract

Status: draft · Branch: `feature/aq-1-queue-cli` off `integrate/phase-aq`
(created from `develop` at phase start; mechanical precondition on the cut
head: `test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`) ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Adds `atm queue` — `atm send` with the nudge deferred until the recipient
harness is ready — together with the taxonomy and code refactor it rests on.
**Nudge** is the umbrella term for post-delivery recipient notification;
**steer** (immediate — today's only kind) and **queue** (deferred) are its
two kinds, used consistently through daemon code.

Verified baseline (integrate/phase-ao2): the steer nudge fires synchronously
post-persistence — `emit_received_hook` call site at
`storage_and_nudge_router.rs:538` (definition :234), guarded only by
`if committed.newly_persisted`, single-call-site pinned by the `al3_*`
architecture test in `boundary_enforcement.rs` (literal string assertions);
no deferral surface exists. `mail_message_states` migrates via
`ensure_column` (`shared_db.rs:888-935`). Real hook-family names:
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
2. **Kind-aware dispatch + mechanical rename pass** (one change set,
   updating the `boundary_enforcement.rs` literal assertions in the same
   commit): `BuiltInPostSendDispatch` (or successor) carries
   `NudgeKind::Steer | Queue`; `ReplacementReceivedHookSelector` routes
   both; `build_built_in_dispatch` (`send/hook.rs:17`) gains the kind
   decision. Renames per the phase-ao2 sweep inventory (recorded in the
   ADR appendix): atm-core boundary family, send family, atm-http-runtime
   router/test doubles, atm-daemon-bootstrap selectors/emitters, atm-graft
   `nudge_sink` family, kind-qualified log/event strings (dedupe the
   `daemon_observability.rs:1084` literal). A terminology grep-gate
   (precedent: `scripts/check-legacy-mailbox-paths.py`) enumerated in CI
   fails new "nudge"-named identifiers where a kind is meant.
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
   that sets `read = 1` also clears `nudge_pending_at` in the same update
   (concrete function named in the PR). Suppression and any queue-kind
   dispatch each emit structured events with `subsystem`/`action`/
   `outcome`.
6. **`PendingNudgeStore`** (owned by `atm-storage`; fixed/internal, ADR-001
   sealed-supertrait pattern; sync methods — async callers use
   `spawn_blocking`):

```rust
/// THE canonical public member key for nudge/queue surfaces, defined in
/// atm-core (per ruthless-boundary-qa direction: one canonical key type,
/// not a per-feature sprawl). Distinct from the PRIVATE
/// runtime_health::MemberKey (runtime_health.rs:43), which is
/// intentionally untouched here; a non-blocking follow-up may migrate it
/// onto this type. AQ2.5's BareCliMemberKey has the identical shape and
/// is superseded by this type — AQ2.5 uses THIS key (see its deliverable
/// 3 note and the derivation at PullPendingReceivedHook::emit).
pub struct MemberKey { pub team: TeamName, pub agent: AgentName }

pub enum NudgeMode { Immediate, Deferred }

pub trait PendingNudgeStore {
    /// Atomically select-and-claim the oldest eligible pending message
    /// (unread, marker set, attempts < ADR-054 (f) max) in ULID order and
    /// clear its marker, returning the attempt number. `None` = nothing
    /// eligible, or another caller won the race. THE at-most-once
    /// mechanism: one conditional UPDATE ... RETURNING, shared verbatim by
    /// the idle-transition drain and the recovery sweep.
    fn claim_next_pending(&self, member: &MemberKey)
        -> Result<Option<NudgeClaim>, StorageError>;
    /// Dispatch of a claimed nudge failed: re-set the marker with
    /// attempts = attempt + 1. At/over the max, the marker stays set but
    /// becomes ineligible for auto-retry and is flagged stuck (the health
    /// signal in ADR-054 (f)).
    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim)
        -> Result<(), StorageError>;
    /// Read-path clear (same state update that sets read = 1).
    fn clear_pending_on_read(&self, member: &MemberKey, msg: &AtmMessageId)
        -> Result<(), StorageError>;
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
        -> Result<(), StorageError>;
}

pub struct NudgeClaim { pub msg: AtmMessageId, pub attempt: u32 }
```

No raw SQL above the backend crate (`no_backend_specific_message_contract`
gate; `message-store.toml`'s closed contract list unaffected).

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
   with `atm send`.
4. Reading a queued message before any nudge clears its marker; daemon
   restart with pending rows re-derives the FIFO (query test).
4a. `claim_next_pending`/`requeue_pending` round-trip: a failed dispatch
   requeues with an incremented attempt; at the ADR-054 (f) max the row
   becomes auto-retry-ineligible and flags stuck (no unbounded retry).
5. Kind-aware dispatch test: Steer and Queue dispatches route through the
   selector; tmux emitter receives Steer only.
6. Boundary governance: ADR-018 §3 amendment + pending-nudge-store TOML +
   `cargo test -p atm-architecture` green + `boundary-guard` review.
7. Terminology grep-gate enumerated in CI. `just test` all three lanes.

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

## Dependencies

- must_follow: none — AQ1 is the phase root.
- parallel_safe: none (AQ2/AQ3 consume the taxonomy, kinds, and store).

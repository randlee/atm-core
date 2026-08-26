# Sprint AQ1 — Implementation Blueprint (3 parallel rust-developer lanes, one shared worktree)

Baseline verified against `integrate/phase-aq` code @ `dc44a36c0` (docs merged at `0aa54ddef`). Line numbers are from the real tree.

## 0. Decision summary

| # | Decision | Anchor |
|---|---|---|
| D1 | `MemberKey`, `NudgeClaim`, `PendingNudgeStore` all live in **`atm-storage`** (option (i)). `atm-core` re-exports. | `crates/atm-storage-rusqlite/Cargo.toml:18-19` |
| D2 | Trait is sealed via the existing `atm_storage::contract::sealed::Sealed` (`crates/atm-storage/src/contract.rs:13-16`), error type is `AtmError`, methods are sync. | `contract.rs:612` (`RosterStore` precedent) |
| D3 | The trait gains **`mark_pending`** (not in the sprint doc — real gap, see §1.4). Without it AC 3 ("state row carries `nudge_pending_at`") is unimplementable without changing `MessageStore` (which AQ1 forbids). |
| D4 | Marker is set in **`PreparedWrite::finish`** (`crates/atm-core/src/send/mod.rs:334`), steer suppression in **`build_received_hook_dispatches`** (`send/mod.rs:391`). Both are existing router call sites (`storage_and_nudge_router.rs:206`, `:210`) → the router body is **not** edited, `al3_*` literals survive. |
| D5 | Read-path clear is implemented **in the whole-row upsert statement** at `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs:28-36`, via `nudge_pending_at = CASE WHEN excluded.read = 1 THEN NULL ELSE mail_message_states.nudge_pending_at END`. One statement covers read/peek-mutating/ack/clear because all of them funnel through `service_runtime_store.rs:294 → save_message → ops.rs:886 insert_initial_message_state`. |
| D6 | FIFO = `ORDER BY message_key` (message keys are `"atm:" + ULID`, `contract.rs:48-53`), so lexicographic order **is** ULID order. No new sort column. |
| D7 | `classify_delivery_channel` takes `GraftLeaseState` (a 2-variant enum owned by AQ1), **not** `Option<&GraftReceiverLease>` — avoids AQ1↔AQ1.5 type-name collision. See §1.6. |
| D8 | Rename breadth cut to `PostSendBuiltInTarget::LocalTmux → LocalSteer` + new `NudgeKind`/`NudgeMode` only; the rest of the phase-ao2 sweep is deferred behind a frozen-inventory grep gate (§4). |

## 1. DECISION 1 — crate placement of `MemberKey` + `PendingNudgeStore`

### 1.1 Verified dependency directions

| Crate | Depends on | Evidence |
|---|---|---|
| `atm-core` (`agent-team-mail-core`) | `atm-error`, **`atm-storage`** | `crates/atm-core/Cargo.toml:23-24` |
| `atm-storage` | `atm-error` only | `crates/atm-storage/Cargo.toml:12-23` |
| `atm-storage-rusqlite` | **`atm-storage` only** — *not* `atm-core` | `crates/atm-storage-rusqlite/Cargo.toml:18-19` |
| `atm-http-runtime`, `atm-daemon-bootstrap` | `atm-core` | `boundaries/atm-http-runtime/http-runtime.toml:30`; `received_hook_selector.rs:12` |

**Decisive:** the backend that must implement `PendingNudgeStore` (`atm-storage-rusqlite`) cannot see `atm-core`. A trait in `atm-core::boundary` would force `atm-storage-rusqlite -> atm-core`, a documented forbidden edge (`boundaries/atm-core/message-received-hook-emitter.toml:24`, `boundaries/atm-storage/nudge-template-override-store.toml:24`). Option (ii) rejected.

Everything the trait needs already lives in `atm-storage`: `TeamName` (`types.rs:454`), `AgentName` (`types.rs:162`), `AtmMessageId` (`schema/inbox_message.rs:22`, re-exported `lib.rs:41`), `AtmError` (`lib.rs:37`), `sealed::Sealed` (`contract.rs:13-16`).

### 1.2 Precedent

`RosterStore` (`contract.rs:612`), `MessageStore` (`:512`), `NudgeTemplateOverrideStore` (`:745`) are all `pub trait X: sealed::Sealed + Send + Sync` in `atm-storage::contract`, implemented per-store in `atm-storage-rusqlite` (`nudge_template_override_store.rs:18-20` shows the `impl atm_storage::contract::sealed::Sealed for …` line), handed out through `StorageHandles` (`atm-storage/src/factory.rs:12-21`), re-exported by `atm-core::boundary` (`atm-core/src/boundary/mod.rs:8-12`). `PendingNudgeStore` does the same.

`MemberKey` goes in `crates/atm-storage/src/types.rs` beside `AgentName`/`TeamName` (domain key, like `PaneId` at `types.rs:771`). The private `atm_http_runtime::runtime_health::MemberKey` (`runtime_health.rs:43`) is untouched in AQ1; note the name collision in the doc comment; consolidation is a non-blocking follow-up.

### 1.3 Canonical contract (author verbatim)

`crates/atm-storage/src/types.rs` (append):

```rust
/// The canonical durable-mailbox member key for nudge and queue surfaces.
///
/// One team-scoped agent identity. This is the key every pending-nudge,
/// drain, sweep, and pump surface uses; features must not define their own
/// per-surface member key. Distinct from the private
/// `atm_http_runtime::runtime_health::MemberKey`, whose migration onto this
/// type is a non-blocking follow-up.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MemberKey {
    pub team: TeamName,
    pub agent: AgentName,
}

impl MemberKey {
    #[must_use]
    pub fn new(team: TeamName, agent: AgentName) -> Self { Self { team, agent } }
    #[must_use]
    pub fn team(&self) -> &TeamName { &self.team }
    #[must_use]
    pub fn agent(&self) -> &AgentName { &self.agent }
}

impl fmt::Display for MemberKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.agent.as_str(), self.team.as_str())
    }
}
```

`crates/atm-storage/src/contract.rs` (append after `NudgeTemplateOverrideStore`, ~line 770):

```rust
/// Maximum automatic delivery attempts for one deferred (queue-kind) nudge.
/// At or above this count the marker stays set but becomes auto-retry
/// ineligible and the row is reported stuck (ADR-054 (f)).
pub const MAX_NUDGE_ATTEMPTS: u32 = 5;

/// One claimed deferred nudge: the message and the failed-attempt count that
/// preceded this claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NudgeClaim {
    pub msg: AtmMessageId,
    /// Value of `nudge_attempts` when the claim was taken. `requeue_pending`
    /// stores `attempt + 1`; `release_pending` leaves it unchanged.
    pub attempt: u32,
}

/// Durable at-most-once delivery state for deferred (`atm queue`) nudges.
/// The store owns one marker column pair on `mail_message_states`; no caller
/// above the backend crate writes SQL. All methods are synchronous.
pub trait PendingNudgeStore: sealed::Sealed + Send + Sync {
    /// Marks one just-admitted message as awaiting a deferred nudge.
    /// Conditional on the row still being unread and not deleted. Returns
    /// whether the marker was set.
    fn mark_pending(&self, member: &MemberKey, msg: &AtmMessageId, at: IsoTimestamp)
        -> Result<bool, AtmError>;
    /// Atomically selects and claims the oldest eligible pending message
    /// (unread, marker set, not deleted, attempts < MAX). ULID order via
    /// message_key. `None` = nothing eligible or lost race. THE at-most-once
    /// mechanism: one conditional `UPDATE … RETURNING`.
    fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError>;
    /// Restores the marker after a failed dispatch: attempts = claim.attempt + 1.
    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError>;
    /// Restores a claim refused for a lifecycle reason (AQ2.7 `agent_blocked`);
    /// attempts unchanged. Conditional/idempotent on claim identity.
    fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError>;
    /// Clears the marker for one message on the read path.
    fn clear_pending_on_read(&self, member: &MemberKey, msg: &AtmMessageId) -> Result<(), AtmError>;
    /// Clears the marker for exactly one just-handed-off message. Unconditional, idempotent.
    fn clear_pending_on_handoff(&self, member: &MemberKey, msg: &AtmMessageId) -> Result<(), AtmError>;
    /// Enumerates members holding at least one eligible pending marker.
    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError>;
}
```

`NudgeMode` does **not** go here — caller-owned write policy per ADR-019, lives in `atm-core::send` beside `WriteRequest`.

### 1.4 Contract gap: `mark_pending`

The sprint's trait has no method that *sets* the marker. Alternatives: (1) field on `Message`/`MessageStore` — rejected (closed contract list); (2) write from router — rejected (router call site frozen); (3) `PendingNudgeStore::mark_pending` from `PreparedWrite::finish` — **selected**. Record in ADR-054 (f): marker write is post-commit and non-transactional with the insert; a crash in that window yields durable-but-never-nudged (same class as today's post-commit steer emission, `storage_and_nudge_router.rs:278-314`). `mark_pending` must be conditional on `read = 0`. Marker-write failure emits `subsystem="atm_core.queue" action="queue_marker_set" outcome="failed"` and must not fail the write.

## 2. Lane split

### 2.0 Shared-worktree protocol

1. **Seam prologue, strictly ordered.** L1 lands *step L1.1 only* (types + trait, no impls), commits, and stops. Then L2 lands *step L2.1 only*, commits, stops. Only then L3's Rust steps start. L3's non-Rust steps (L3.1–L3.3) start immediately.
2. **Scoped checks during the interleaved window.** `cargo check -p <own crates>` (`-p atm-storage -p atm-storage-rusqlite` / `-p agent-team-mail-core -p atm-graft` / `-p agent-team-mail -p atm-daemon-bootstrap`), not `--workspace`, until all lanes finish. Full `just test` runs once at the end, by L3.
3. Each lane commits only its own files (`git add <paths>`), never `git add -A`. Never rebase/reset; never touch another lane's files.

### 2.1 File ownership map (exhaustive)

| File | Lane | Notes |
|---|---|---|
| `crates/atm-storage/src/types.rs` | L1 | `MemberKey` |
| `crates/atm-storage/src/contract.rs` | L1 | trait, `NudgeClaim`, `MAX_NUDGE_ATTEMPTS` |
| `crates/atm-storage/src/lib.rs` | L1 | re-exports |
| `crates/atm-storage/src/factory.rs` | L1 | `pending_nudge_store` handle |
| `crates/atm-storage-rusqlite/src/shared_db.rs` | L1 | DDL + `ensure_*` + partial index |
| `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs` | L1 | read-path clear |
| `crates/atm-storage-rusqlite/src/pending_nudge_store.rs` | L1 | **new** |
| `crates/atm-storage-rusqlite/src/lib.rs` | L1 | `mod`, struct decl, factory wiring at `:681` |
| `crates/atm-core/src/service_runtime.rs` | L1 | `with_pending_nudge_store` + accessor (L2/L3 code against it, never edit it) |
| `boundaries/atm-storage/pending-nudge-store.toml` | L1 | **new** |
| `docs/atm-storage/boundaries.md` | L1 | new section (required by `.just/lint-config.toml:110-112`) |
| `crates/atm-architecture/tests/pending_nudge_store_boundary.rs` | L1 | **new** |
| `crates/atm-core/src/boundary/mod.rs` | L2 | `NudgeKind`, `LocalSteer`, re-export of L1 types |
| `crates/atm-core/src/send/mod.rs` | L2 | `NudgeMode`, `finish`, `build_received_hook_dispatches` |
| `crates/atm-core/src/send/hook.rs` | L2 | kind decision |
| `crates/atm-core/src/nudge_dispatch.rs` | L2 | **new** — dispatch-from-message-id |
| `crates/atm-core/src/delivery_channel.rs` | L2 | **new** — backend enum + classifier |
| `crates/atm-core/src/lib.rs` | L2 | two `mod` lines |
| `crates/atm-core/src/graft.rs` | L2 | `LocalSteer`/`kind` compile fixes only |
| `crates/atm-core/src/delivery_policy.rs` | L2 | visibility only |
| `crates/atm-graft/src/nudge_sink.rs`, `crates/atm-graft/src/runtime.rs` | L2 | `kind:` field on literals (`nudge_sink.rs:200`, `runtime.rs:738`) |
| `crates/atm-http-runtime/src/storage_and_nudge_router.rs` | L2 | **`mod tests` only** (`:908-909`, `:1268-1277`, `:2819-2821`, `:2167`, `:2395`). Production body ≤ `:696` is frozen. |
| `crates/atm/src/commands/send.rs` | L3 | `run_with_mode` fork |
| `crates/atm/src/commands/queue.rs` | L3 | **new** |
| `crates/atm/src/commands/mod.rs` | L3 | `Queue` variant (`:104-152`) |
| `crates/atm/tests/cli_surface_baseline.json` | L3 | regen |
| `crates/atm-daemon-bootstrap/src/received_hook_selector.rs` | L3 | kind-aware routing + tests |
| `crates/atm-daemon-bootstrap/src/lib.rs` | L3 | test double at `:957-963` |
| `crates/atm-architecture/tests/boundary_enforcement.rs` | L3 | `al3_*` literal updates |
| `boundaries/atm-core/message-received-hook-emitter.toml` | L3 | manifest currency |
| `docs/atm-core/boundaries.md` | L3 | emitter section sync |
| `scripts/check-nudge-taxonomy.py` | L3 | **new** gate |
| `Justfile`, `.just/run_lint.py`, `.just/tests/test_run_lint.py` | L3 | gate wiring |
| `docs/adr/ADR-054-*.md`, `docs/adr/INDEX.md` | L3 | ADR |

**Arbitrated shared files:**

| File | Assigned | Interface the other lane codes against |
|---|---|---|
| `crates/atm-core/src/boundary/mod.rs` | **L2** | L2 adds `#[doc(inline)] pub use atm_storage::contract::{MemberKey, NudgeClaim, PendingNudgeStore};` in L2.1. L1 imports `atm_storage::…` directly. |
| `crates/atm-core/src/service_runtime.rs` | **L1** | L2/L3 call `runtime.pending_nudge_store() -> Result<Arc<dyn PendingNudgeStore + Send + Sync>, AtmError>` (shape of `async_message_search_store()` at `:222-230`). Builder `with_pending_nudge_store(self, store) -> Self` (shape of `:200-207`). L1 lands this in L1.1. |
| `crates/atm-architecture/tests/boundary_enforcement.rs` | **L3** | L1's boundary assertion goes in a new sibling test file. |
| `crates/atm-http-runtime/src/storage_and_nudge_router.rs` | **L2** (test module only) | L3 never opens this file. |

### 2.2 Lane L1 — storage

**L1.1 — seam prologue (blocks L2/L3; do this first, commit, stop).**
- `types.rs`: `MemberKey` (§1.3).
- `contract.rs`: `MAX_NUDGE_ATTEMPTS`, `NudgeClaim`, `PendingNudgeStore` (§1.3), plus a `DummyPendingNudgeStore` in `mod tests` (`:772`) so `storage_traits_are_object_safe` (`:896`) covers it.
- `atm-storage/src/lib.rs`: extend `pub use types::{…}` (`:66`) with `MemberKey`; `pub use contract::{…}` (`:27-36`) with `NudgeClaim, PendingNudgeStore, MAX_NUDGE_ATTEMPTS`.
- `atm-core/src/service_runtime.rs`: optional field + `with_pending_nudge_store` + `pending_nudge_store()` mirroring `:136`/`:200-230`; add to `Debug` impl at `:387`.

**L1.2 — schema.** `shared_db.rs`: in `DB_MIGRATIONS` `CREATE TABLE IF NOT EXISTS mail_message_states` (`:57-71`) add `nudge_pending_at TEXT NULL, nudge_attempts INTEGER NOT NULL DEFAULT 0,`. New `fn ensure_mail_message_states_nudge_columns(connection, target)` beside `ensure_team_nudge_template_override_columns` (`:844`), two `ensure_column` calls (`:888`). **Partial index created inside that function after the `ensure_column` calls, NOT in `DB_MIGRATIONS`** (`ensure_schema` `:650-663` runs `DB_MIGRATIONS` at `:656` before column migrations):
```sql
CREATE INDEX IF NOT EXISTS idx_mail_message_states_pending
    ON mail_message_states(team, agent, message_key)
    WHERE nudge_pending_at IS NOT NULL;
```
Call from `ensure_schema` after `:663`.

**L1.3 — read-path clear.** `writer/stmt_cache.rs:26-37`: extend `ON CONFLICT … DO UPDATE SET` with (no new bind):
```sql
  nudge_pending_at = CASE WHEN excluded.read = 1
                          THEN NULL
                          ELSE mail_message_states.nudge_pending_at END
```
Doc-comment rationale: the conflict path is the only way a state row transitions to `read = 1` (`atm-core/src/read/mod.rs:659` → `service_runtime_store.rs:273-294` → `writer/ops.rs:886-912`). `nudge_attempts` deliberately preserved.

**L1.4 — backend impl.** New `atm-storage-rusqlite/src/pending_nudge_store.rs` modelled on `nudge_template_override_store.rs` (`self.db.with_connection(|c| …)`, `impl atm_storage::contract::sealed::Sealed for …`). `struct SqlitePendingNudgeStore { db: Arc<SharedDb> }` in `lib.rs`; `mod pending_nudge_store;`; construct in `StorageHandles::from_parts` at `lib.rs:681`. Statements:
```sql
-- mark_pending (rows_changed == 1)
UPDATE mail_message_states SET nudge_pending_at = ?4, nudge_attempts = 0, updated_at = ?4
 WHERE team = ?1 AND agent = ?2 AND message_key = ?3 AND read = 0 AND deleted_at IS NULL;
-- claim_next_pending
UPDATE mail_message_states SET nudge_pending_at = NULL, updated_at = ?4
 WHERE rowid = (SELECT rowid FROM mail_message_states
                 WHERE team = ?1 AND agent = ?2 AND nudge_pending_at IS NOT NULL
                   AND read = 0 AND deleted_at IS NULL AND nudge_attempts < ?3
                 ORDER BY message_key ASC LIMIT 1)
RETURNING message_key, nudge_attempts;
-- requeue_pending (?5 = claim.attempt+1, ?6 = claim.attempt)
UPDATE mail_message_states SET nudge_pending_at = ?4, nudge_attempts = ?5, updated_at = ?4
 WHERE team = ?1 AND agent = ?2 AND message_key = ?3 AND nudge_pending_at IS NULL AND nudge_attempts = ?6;
-- release_pending (?5 = claim.attempt)
UPDATE mail_message_states SET nudge_pending_at = ?4, updated_at = ?4
 WHERE team = ?1 AND agent = ?2 AND message_key = ?3 AND nudge_pending_at IS NULL AND nudge_attempts = ?5;
-- clear_pending_on_read / clear_pending_on_handoff
UPDATE mail_message_states SET nudge_pending_at = NULL, updated_at = ?4
 WHERE team = ?1 AND agent = ?2 AND message_key = ?3;
-- list_pending_members
SELECT DISTINCT team, agent FROM mail_message_states
 WHERE nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL;
```
Use `MessageKey::from(AtmMessageId)` (`contract.rs:56-60`) for the bind value.

**L1.5 — boundary governance.** `boundaries/atm-storage/pending-nudge-store.toml` shaped like `nudge-template-override-store.toml`: `owner_package = "atm-storage"`, `allowed_dependents = ["atm-core", "atm-daemon-bootstrap", "atm-http-runtime", "atm-storage-rusqlite"]`, `forbidden_edges = ["atm-storage -> atm-core", "atm-storage -> atm-storage-rusqlite", "atm-storage -> atm-daemon"]`, `io_forbidden = ["direct_sqlite_io", "message_delivery", "process_spawn"]`, `error_types = ["AtmError"]`, `state = "concrete_landed"`. Matching section in `docs/atm-storage/boundaries.md`.

**L1.6 — tests.** `pending_nudge_store.rs` `mod tests` (in-memory DB precedent `shared_db.rs:1128`): mark→claim; two concurrent claims → one `Some` one `None`; requeue increments and at MAX claim returns `None` with marker set (AC 4a); release leaves attempts; handoff clear on backlog clears the named message not the oldest; `list_pending_members` excludes read/deleted (AC 8); FIFO across three ULIDs (AC 4). `shared_db.rs` legacy-DB migration test (style `:1209`). New `atm-architecture/tests/pending_nudge_store_boundary.rs`. Read-clear test: marked row, upsert `read = 1`, assert NULL.

### 2.3 Lane L2 — core send/dispatch

**L2.1 — seam prologue (after L1.1; commit, stop).** `boundary/mod.rs`:
```rust
#[doc(inline)]
pub use atm_storage::contract::{MAX_NUDGE_ATTEMPTS, NudgeClaim, PendingNudgeStore};
#[doc(inline)]
pub use atm_storage::types::MemberKey;

/// Which kind of recipient nudge a committed dispatch represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeKind { Steer, Queue }
```
and at `:150-159`: `PostSendBuiltInTarget::LocalTmux → LocalSteer(LocalTmuxNudgeTarget)`; `BuiltInPostSendDispatch` gains `pub kind: NudgeKind`.
`send/mod.rs` beside `WriteRequest` (`:112`):
```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeMode { #[default] Immediate, Deferred }
```
plus `#[serde(default)] pub nudge_mode: NudgeMode` on `WriteRequest` and `with_nudge_mode` builder; `WriteRequest::new` (`:160`) signature unchanged. Fix construction sites: `send/hook.rs:32-38`, `:42-50`; `graft.rs:89`, `:116` (match arm only — `GraftPostSendRequest` `:44-51` wire unchanged); `atm-graft/src/nudge_sink.rs:67`/`:200-203`; `atm-graft/src/runtime.rs:738-741` (`kind: NudgeKind::Steer`); `storage_and_nudge_router.rs` **test module only**.

**L2.2 — kind decision + suppression.** `send/hook.rs:17-53`: `build_built_in_dispatch` gains `nudge_mode: NudgeMode`, stamps `kind`. `send/mod.rs:391-417`: first statement — if `Deferred`, `tracing::info!(subsystem="atm_core.queue", action="steer_suppressed", outcome="ok", …)` and `return Ok(Vec::new())`. `send/mod.rs:334-340` `finish`: when `is_newly_persisted() && Deferred`, resolve `MemberKey` and call `runtime.pending_nudge_store()?.mark_pending(…)`; failure logged (`action="queue_marker_set" outcome="failed"`), never propagated.

**L2.3 — classifier seam.** New `atm-core/src/delivery_channel.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalMessageReceivedBackend { Tmux { pane_id: PaneId }, Herdr }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraftLeaseState { Absent, Active }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryChannel { TmuxSteer, HerdrSteer, Graft, BareCli }
#[must_use]
pub fn classify_delivery_channel(local_backend: Option<&LocalMessageReceivedBackend>, graft_lease: GraftLeaseState) -> DeliveryChannel {
    match (local_backend, graft_lease) {
        (Some(LocalMessageReceivedBackend::Tmux { .. }), _) => DeliveryChannel::TmuxSteer,
        (Some(LocalMessageReceivedBackend::Herdr), _) => DeliveryChannel::HerdrSteer,
        (None, GraftLeaseState::Active) => DeliveryChannel::Graft,
        (None, GraftLeaseState::Absent) => DeliveryChannel::BareCli,
    }
}
/// Derives the local backend from durable roster data, no schema change:
/// `recipient_pane_id` → Tmux; `metadata_json["backendType"] == "herdr"` → Herdr; else None.
#[must_use]
pub fn local_message_received_backend(member: &crate::boundary::RosterEntry) -> Option<LocalMessageReceivedBackend>
```
No schema migration: `RosterEntry` = `atm_storage::contract::RosterMember` (`boundary/store.rs:35`), `recipient_pane_id` (`contract.rs:460`), `metadata_json` (`:462`) already persisted. Add agreement note at `delivery_policy.rs:78-83`.

**L2.4 — dispatch-from-message-id.** New `atm-core/src/nudge_dispatch.rs` (outside `send/` — `boundary_enforcement.rs:2881-2885` asserts the planner never reloads):
```rust
pub fn rebuild_received_hook_dispatch(runtime: &LocalServiceRuntime, member: &MemberKey, message_id: AtmMessageId, kind: NudgeKind)
    -> Result<Option<BuiltInPostSendDispatch>, AtmError>
```
`MessageKey::from(message_id)` → `runtime.message_store.load_message` filtered on team/agent → `PostSendHookEvent` (mapping as `send/hook.rs:99-121`) → roster lookup → `DeliveryRecipientSnapshot::from_roster` → `hook::build_built_in_dispatch`.

**L2.5 — tests (atm-core only).** `send/tests.rs`: Deferred → zero dispatches + exactly one `mark_pending` on a recording double; Immediate byte-identical; duplicate write → zero `mark_pending`. `delivery_channel.rs`: 4-row table + `local_message_received_backend` cases. `nudge_dispatch.rs`: rebuild == write-time dispatch (AC 8).

### 2.4 Lane L3 — CLI, selector, gates, ADR

**L3.1 — terminology grep gate (t=0).** New `scripts/check-nudge-taxonomy.py` like `scripts/check-legacy-mailbox-paths.py` (frozen dataclass tables, `--repo-root`, `as_posix()`, exit 0/1). Frozen inventory allowlist `ALLOWED_NUDGE_IDENTIFIERS` generated from `rg -o '[A-Za-z_]*[Nn]udge[A-Za-z_]*' crates | sort -u`; `FORBIDDEN_PATTERNS` for `PostSendHookEmitter`, `PostSendBuiltInTarget::LocalTmux`. Any new nudge-identifier not in inventory fails. Wire: `Justfile` after `:93`, `.just/run_lint.py` after `:152`, `.just/tests/test_run_lint.py:37-57`.

**L3.2 — emitter manifest (t=0).** `boundaries/atm-core/message-received-hook-emitter.toml` `[status].notes` (`:46`): replace with real set — `TokioTmuxReceivedHook` (`received_hook_selector.rs:120`), `PublishedGraftReceivedHook` (`:220`), sync `atm_graft::nudge_sink::GraftReceiveHook` (`nudge_sink.rs:64`); state it is the authoritative implementer count later sprints extend. Add `AsyncMessageReceivedHookEmitter` to `[public]`. Sync `docs/atm-core/boundaries.md`.

**L3.3 — ADR-054 (t=0).** `docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md` closing (a)–(g) + `docs/adr/INDEX.md`. Also record: D1 evidence; `mark_pending` + crash window; `GraftLeaseState` deviation (D7) + AQ1.5 amendment; `MAX_NUDGE_ATTEMPTS = 5`; deferred rename inventory (§4) + gate; §3.2 invariant closure; §3.6 blocking-in-finish acceptance.

**L3.4 — `atm queue` (after L2.1).** `atm/src/commands/send.rs`: `run` → `run_with_mode(obs, NudgeMode::Immediate)`; `run_with_mode` passes mode into `build_request` → `.with_nudge_mode(mode)` (`:185-191`); `resolve_command_runtime_context("send" | "queue")`. New `atm/src/commands/queue.rs`: `QueueCommand { #[command(flatten)] inner: SendCommand }` → `run_with_mode(Deferred)`. `commands/mod.rs`: `pub use`, `Queue(QueueCommand)`, dispatch arm. Regenerate `crates/atm/tests/cli_surface_baseline.json` (`atm __dump-cli-surface --format json`) and versioned CLI docs via `crates/atm/examples/gen_cli_docs.rs`. Run `crates/atm/tests/openapi_surface.rs` — `nudge_mode` should NOT appear in the HTTP DTO; a diff there is a finding.

**L3.5 — kind-aware selector (after L2.1).** `received_hook_selector.rs:85-95`:
```rust
match (dispatch.kind, &dispatch.target) {
    (NudgeKind::Steer, PostSendBuiltInTarget::LocalSteer(_)) => Some(&self.tmux),
    (NudgeKind::Steer, PostSendBuiltInTarget::Graft(_)) => Some(&self.graft),
    (NudgeKind::Queue, _) => None, // AQ2/AQ3 own queue-kind emitters
}
```
No spawn/channel (`boundary_enforcement.rs:2871-2880`). Tests: `queue_dispatch()` → `None`; tmux only for Steer (AC 5). Fix test double `atm-daemon-bootstrap/src/lib.rs:957-963`.

**L3.6 — architecture literals.** `boundary_enforcement.rs`: literals at `:2841/:2844/:2858/:2863/:2882` should NOT need changing (router untouched). Add: `send_module_code.contains("NudgeMode::Deferred")`, `received_hook_selector.contains("NudgeKind::Queue")`.

**L3.7 — full gates.** `just test`, `cargo test -p atm-architecture`, `just lint`, hermes-atm Python tests.

## 3. Integration risks

- **3.1 `al3_*` literals** (`boundary_enforcement.rs:2827-2916`): kept intact by D4 + `nudge_dispatch.rs` placement. L2 edits nothing in the router above `:696`. A production-body edit there is a design regression — stop.
- **3.2 `http-runtime.toml:41` invariant**: closed, no manifest edit — deferred write yields an empty dispatch vector, already legal (`send/mod.rs:397`). Say so in ADR-054 (c).
- **3.3 `newly_persisted` guard**: `mark_pending` must be gated on `is_newly_persisted()` (`send/mod.rs:365-367`) or peer receipt/replay re-marks read messages. Test it.
- **3.4 hermes-atm `PyNudge`**: `PyNudge::from_post_send` (`atm-graft-python/src/lib.rs:1063`) uses `PostSendHookEvent`, untouched. `GraftPostSendRequest` built field-by-field (`graft.rs:116-120`) — `kind` cannot leak into wire JSON. Only `atm-graft/src/runtime.rs:738` changes.
- **3.5 Windows**: `UPDATE … RETURNING` needs SQLite ≥ 3.35 (bundled-windows + modern_sqlite, `atm-storage-rusqlite/Cargo.toml:30-31`) — ensure the claim test is not `#[cfg(unix)]`. Lint script: pathlib/`as_posix()`, explicit UTF-8, `{{python_cmd}}`, ASCII output. No test shells out to tmux.
- **3.6a Blocking in `commit_write`**: `mark_pending` is a sync SQLite write in `prepared.finish` — same class as the existing ack transition; record as accepted in ADR-054.
- **3.6b `StorageHandleParts` struct-literal break**: single in-tree constructor (`lib.rs:681`, L1). Check `boundaries/hermes-atm/runtime-composition.toml`; if hermes constructs it, make the field `Option<…>` with an accessor.

## 4. Deferred out of AQ1

| Defer | Why |
|---|---|
| Full phase-ao2 rename sweep (router/test-double, selector/emitter, `nudge_sink` family, log strings) | AC 2 fixes only that the change set compiles with assertions updated; breadth multiplies 3-way conflicts. Keep `LocalSteer` + two enums; frozen-inventory gate prevents new ambiguity. Record residual inventory in ADR-054 appendix → AQ2.6/AQ3. |
| `daemon_observability.rs:1084` dedupe | Legacy daemon; CLAUDE.md forbids patching. Strike from sprint doc. |
| Renaming `LocalTmuxNudgeTarget` payload | Payload is tmux-shaped; AQ2.6 needs the two-armed payload. |
| Migrating `runtime_health::MemberKey` | Sprint says untouched; private. |
| `GraftReceiverLease` as classifier param | Replaced by `GraftLeaseState` (D7); AQ1.5 doc needs a one-line amendment. |
| `--attach` parity | Lane C; `run_with_mode` + flatten deliver it free. |

## 5. Build sequence

```
L1.1 seam ──► L2.1 seam ──► ┌ L2.2 ──► L2.4 ──► L2.5
                            └ L3.4 ──► L3.5 ──► L3.6 ──► L3.7
L1.2 ─► L1.3 ─► L1.4 ─► L1.5 ─► L1.6          (after L1.1, parallel with L2/L3)
L3.1 / L3.2 / L3.3                             (t=0, no Rust dependency)
```

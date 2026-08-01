# Phase AJ Sprint Plan: Agent Session State & Heartbeat Extension

**Date:** 2026-08-01
**Branch:** `plan/phase-aj`
**Source research:** `docs/plans/phase-aj/phase-aj-research.md`
**Source codebase:** atm-core `develop` at `a6cb4587`
**Author:** rust-architect (Hermes profile)

---

## 0. Reading Guide — How To Use This Plan

Each sprint below is self-contained. A fresh agent must be able to pick up any single
sprint card, read the sprint section plus its "Context" subsection, and implement
without asking the human. Every sprint declares:

- **Blocks:** what this sprint must land before.
- **Blocked-by:** what must land before this sprint can start.
- **Justification:** *why* the design decision was made (not just *what*).
- **Files touched:** exact paths in the repo.
- **Tests:** named test cases that must exist when the sprint closes.
- **Acceptance criteria:** observable, binary statements that are true when done.

Hard constraints (apply to every sprint below — never relaxed):

1. **No behavioral branching on session_id / pid presence.** These fields are
   observational state only — recorded, never acted upon. No code path may change
   behavior based on `Option::is_some()` of either field. The compiler should be
   the enforcement mechanism (no `if let Some(session_id) = ...` outside of
   cache-update code that *stores* the value).
2. **Absent fields don't overwrite.** `None` in an incoming request leaves the
   cached value untouched. Only `Some(v)` updates state. This applies to both
   update paths (UDS dispatch and HTTP heartbeat) and to both `session_id` and
   `pid`.
3. **In-memory only.** No SQLite schema change. No persistence of `session_id`,
   `pid`, or any new runtime state. Daemon restart loses this data — by design.
4. **Two update paths, one storage mechanism.** `RuntimeStatusCache` is the only
   writer. Both UDS dispatch and HTTP heartbeat call into the same cache methods;
   they do not duplicate state-merging logic.

---

## 1. Goal Statement (Why This Phase Exists)

The daemon already maintains a `RuntimeStatusCache` of per-member liveness
(`Active / Idle / Offline / Unknown / IdentityConflict`) fed by HTTP heartbeats.
Today, the cache only knows `pid` and `last_active_at`. Two gaps prevent
downstream tooling (graft, oversight, status dashboards) from reasoning about
*which agent session* is live:

- **Gap A — CLI traffic is invisible.** `atm send`, `atm read`, and `atm ack`
  traverse the daemon over UDS but never touch the runtime cache. An agent that
  only sends/reads mail (never posts a heartbeat) shows up as `Unknown` in
  snapshots.
- **Gap B — no session identifier.** Multiple Claude Code sessions can share an
  `ATM_IDENTITY`. Without a `session_id`, the daemon cannot distinguish "agent
  restarted" from "second session for same agent" and cannot let oversight
  tooling address a specific session.

Phase AJ closes both gaps by threading two *optional, observational* fields
(`session_id`, `pid`) through the existing UDS wire payloads and the existing
HTTP heartbeat payload, and by extending the in-memory cache to record them
under the non-overwrite rule.

**Out of scope** (explicit non-goals, do not implement):

- Persisting `session_id` to SQLite. Deliberately ephemeral.
- Authorization, rate-limiting, or behavior changes based on `session_id`.
- Any new wire endpoint. We extend existing payloads only.
- Hooks / sc-hooks integration. Hooks will *set* `ATM_SESSION_ID` / `ATM_PID`,
  but that work belongs to the sc-hooks repo, not atm-core.

---

## 2. Architecture Decision (AD-AJ-1)

### 2.1 The Decision

**Extend existing payloads with optional fields, do not introduce a new wire
message.** Specifically:

- `WriteRequest` gains `session_id: Option<SessionId>` and `pid: Option<u32>`.
- `ReadQuery` gains `session_id: Option<SessionId>` and `pid: Option<u32>`.
- `TeamMemberHeartbeatRequest` gains `session_id: Option<SessionId>`.
- `TeamMemberHeartbeatResponse` gains `session_id: Option<SessionId>` (echoed
  back so the caller can confirm what was cached).
- `RuntimeStatusSnapshot` gains a `session_id: Option<SessionId>` per member.
- New `SessionId` newtype in `atm-core`, owned by `types.rs`.

All new fields are `#[serde(default, skip_serializing_if = "Option::is_none")]`
so old clients and new daemons interoperate cleanly in both directions.

### 2.2 Why this shape (and not the alternatives)

**Why not a separate "session registration" RPC?** A new RPC would force every
CLI command to either (a) make two round trips (register + actual command) or
(b) be fire-and-forget on registration. Both are worse: (a) doubles latency on
every `atm read`, (b) loses the cache touch when the registration races the
actual command. Optional sidecar fields piggyback on traffic the daemon
already serves — zero extra syscalls in the common case.

**Why not require session_id?** Backwards compatibility. Existing CLI binaries,
hook scripts, and third-party callers would all break on a required field.
Making it optional with the non-overwrite rule means old callers keep working
forever, and the cache just stays at whatever value it last saw.

**Why in-memory only?** Session state is by definition tied to a daemon
lifetime. If the daemon restarts, every agent must re-announce itself anyway
(via heartbeat or next CLI call). Persisting ephemeral session state would
create a staleness hazard on daemon restart with no compensating benefit —
the very first CLI command after restart re-populates the cache.

**Why a newtype `SessionId` instead of `String`?** Per the Rust API Guidelines
(C-NEWTYPE) and the workspace's existing pattern (`AgentName`, `TeamName`,
`ChatId`). A newtype prevents accidental interchange with arbitrary strings,
documents the semantic, and gives us a single place to add validation later
(e.g. max length, character whitelist) if it ever becomes necessary.

**Why echo `session_id` in `TeamMemberHeartbeatResponse`?** Symmetric with
`pid`, which is already echoed. The hook process that posts the heartbeat has
no other way to confirm what the daemon cached; without an echo it cannot
distinguish "cached" from "silently dropped".

### 2.3 The non-overwrite rule, formalized

For every cache field `f ∈ {session_id, pid}` and every incoming request `R`:

```
if R.f is Some(v)  →  cache.f := Some(v)
if R.f is None     →  cache.f unchanged
```

This rule is implemented **once**, in `RuntimeStatusCache`, never duplicated
at the dispatcher or HTTP layer. See §3.4 for the merge-point design.

### 2.4 The "no behavioral branching" constraint, formalized

The only code allowed to inspect `Option::is_some()` on `session_id` / `pid`
is the cache-merge code that *stores* the value (§3.4). Anywhere else, these
fields must flow as opaque data. The compile-time enforcement mechanism is
that `SessionId` exposes no predicate methods; reviewers must grep for
`session_id.is_some()` / `session_id.is_none()` outside of the cache module
during code review and reject any match.

---

## 3. Component Design

### 3.1 `SessionId` — new type

**File:** `crates/atm-core/src/types.rs` (extend, do not create a new module —
session is a peer concept to agent/team/chat identity).

```rust
/// Stable identifier for one agent session, scoped to one team member.
///
/// Opaque to the daemon — never inspected, only stored and echoed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);
```

Provide `SessionId::new(impl Into<String>)`, `as_str()`, and `Display`. Do
**not** provide `is_empty()` or any other predicate that would invite
behavioral branching. Validation (max length, charset) is deferred — adding
it later is a non-breaking change because the type is already opaque.

**Justification:** matches the established pattern for `AgentName`,
`TeamName`, `ChatId` in the same file. Keeping it in `types.rs` (rather than
a new `session.rs`) avoids a new module for a single newtype, which would
violate the "modules should carry their weight" principle from the workspace
guidelines.

### 3.2 Protocol extensions

**File:** `crates/atm-core/src/protocol.rs`

- `TeamMemberHeartbeatRequest` gains `session_id: Option<SessionId>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- `TeamMemberHeartbeatResponse` gains `session_id: Option<SessionId>` with
  the same serde attributes.
- `RuntimeMemberState` — **no change** (already covers the full lifecycle).
- `RuntimeStatusSnapshot` — **structural change**: introduce a new nested
  struct `RuntimeMemberStatus { state, last_active_at, pid, session_id }` and
  replace `member_counts` with `members: Vec<RuntimeMemberStatus>`. *This is
  a breaking change to the snapshot schema* — see §3.6 for migration notes.

**Justification for the snapshot restructure:** today the snapshot only
carries *counts*. To surface per-member `session_id` (the entire point of
this phase), the snapshot must carry per-member records. The aggregate
`member_counts` can be recomputed by the consumer, so we drop it from the
wire to keep the schema minimal — one source of truth.

### 3.3 Wire payloads

**Files:** `crates/atm-core/src/send/mod.rs`, `crates/atm-core/src/read/mod.rs`

Both `WriteRequest` and `ReadQuery` gain:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub session_id: Option<SessionId>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub pid: Option<u32>,
```

**Justification for adding `pid` even though the daemon already gets it via
heartbeat:** the heartbeat path only fires on a timer or hook event. A CLI
command may be the *first* contact an agent has with the daemon after a
restart, before any heartbeat has fired. Without `pid` on the wire payloads,
that first command cannot populate the cache and the agent remains `Unknown`
until the next heartbeat.

### 3.4 Cache storage and merge logic

**File:** `crates/atm-daemon/src/runtime_status_cache.rs`

Extend `RuntimeMemberRecord`:

```rust
struct RuntimeMemberRecord {
    pid: Option<u32>,
    session_id: Option<SessionId>,   // NEW
    state: RuntimeMemberState,
    last_active_at: Option<IsoTimestamp>,
}
```

Add one new method — the **single merge point** for both update paths:

```rust
pub(crate) fn touch_member(
    &self,
    team: &TeamName,
    member: &AgentName,
    session_id: Option<&SessionId>,
    pid: Option<u32>,
    observed_at: IsoTimestamp,
);
```

Semantics:
- If the member is not yet in the cache, insert with `state = Unknown`,
  `last_active_at = Some(observed_at)`, and the provided `session_id` / `pid`.
- If the member is in the cache, apply the non-overwrite rule:
  `session_id` updates only if `Some`, `pid` updates only if `Some`,
  `last_active_at` always advances to `observed_at`.
- `state` is **not** modified by `touch_member` — heartbeat remains the only
  writer of state transitions.

**Justification for a separate `touch_member` instead of reusing
`record_heartbeat`:** heartbeat *sets* state; touch *observes* presence.
Conflating them would force UDS dispatch to fabricate a `HeartbeatActivity`
value it does not actually know. Keeping the two methods distinct preserves
the invariant that `RuntimeMemberState` transitions only on heartbeat events.

`record_heartbeat` is extended to also store `session_id` (same
non-overwrite rule) and `cached_session_id(team, member)` is added as an
accessor.

### 3.5 Dispatcher integration

**File:** `crates/atm-daemon/src/runtime_health.rs`

Two touch points, both after the underlying operation succeeds:

- `route_write()` — after `MessageWriter::write` returns `Ok`, call
  `status_cache.touch_member(&request.caller_team, &request.caller_identity,
  request.session_id.as_ref(), request.pid, IsoTimestamp::now())`.
- `dispatch_non_write()` for `RequestEnvelope::Receive` — after
  `read_mail_with_runtime` returns `Ok`, same touch with the `ReadQuery`'s
  caller fields.

`record_heartbeat()` (the HTTP path) is unchanged in shape; it just passes
`request.session_id` through to the cache.

**Justification for touching *after* success, not before:** a failed send or
read should not mark the caller as observed. Touching on failure would
conflate "agent is alive but its message failed validation" with "agent is
alive and well". The cache tracks *liveness*, not *attempts*.

**Justification for not touching on `Ack`:** `atm ack` already traverses
`route_write` (it builds a `WriteRequest` internally — see
`runtime_health.rs:560` and `send/mod.rs:100-104`). No separate code path is
needed.

### 3.6 Snapshot schema migration

`RuntimeStatusSnapshot` changes shape. Old daemon → new client and new
daemon → old client must both fail cleanly.

- The snapshot is consumed by `atm doctor` and `atm status` (both in
  `crates/atm/src/output.rs:747`).
- These consumers will be updated in the same phase to read the new shape.
- The daemon's compatibility preflight already versions the protocol
  (`CompatibilityPreflight` in `protocol.rs:186`); we bump
  `CLI_SCHEMA_VERSION` from `1` to `2` to signal the breaking change.

**Justification:** the snapshot is an internal observability payload, not a
stable public API. Bumping the schema version is the cheapest correct
migration — it forces a clean version handshake failure rather than silent
misinterpretation.

### 3.7 CLI caller context

**File:** `crates/atm-core/src/caller_context.rs`

- `CallerContext` gains `session_id: Option<SessionId>` and
  `pid: Option<u32>`.
- New public functions:
  - `read_cli_session_id_from_env() -> Result<Option<SessionId>, AtmError>`
    reads `ATM_SESSION_ID`.
  - `read_cli_pid_from_env() -> Result<Option<u32>, AtmError>` reads
    `ATM_PID`. If `ATM_PID` is unset, fall back to `std::process::id()` —
    the actual OS pid of the CLI process is almost always the right answer.
- `resolve_cli_inspection_caller_context` and
  `resolve_cli_mutation_caller_context_with_overrides` populate the new
  fields.

**Justification for the `std::process::id()` fallback:** the goal is "the
daemon knows which process is talking to it". When `ATM_PID` is unset, the
CLI process's own pid is the most accurate available value. Agents that want
to report a *different* pid (e.g. a hook reporting on behalf of its parent)
set `ATM_PID` explicitly. This is the same "sensible default, overridable"
pattern used elsewhere in the workspace (see `ATM_IDENTITY` handling).

**Justification for adding to `CallerContext` rather than passing as
separate function args:** `CallerContext` is the existing carrier of "who is
calling" through the CLI command layer. Adding the new fields there means
`send.rs`, `read.rs`, `ack.rs` only need mechanical changes — pull the
fields off the context they already hold — rather than threading two new
parameters through every call site.

### 3.8 HTTP API surface

**File:** `crates/atm-core/src/api.rs`

`HEARTBEAT_PATH` is unchanged (`/v1/atm/heartbeat`). The request/response
DTOs in §3.2 flow through serde automatically — no router changes needed.

---

## 4. Data Flow

### 4.1 UDS path (`atm send` / `atm read` / `atm ack`)

```
shell env: ATM_IDENTITY, ATM_TEAM, ATM_SESSION_ID?, ATM_PID?
        │
        ▼
caller_context::resolve_cli_*_caller_context()
        │ CallerContext { identity, team, chat_id, session_id, pid }
        ▼
commands::send / read / ack ::build_request()
        │ WriteRequest / ReadQuery { ..., session_id, pid }
        ▼
UDS transport → daemon
        │
        ▼
DaemonRequestDispatcher::route_write / dispatch_non_write
        │ underlying op succeeds
        ▼
RuntimeStatusCache::touch_member(team, member, session_id, pid, now)
        │ non-overwrite rule for session_id & pid
        ▼
RuntimeMemberRecord updated (in-memory only)
```

### 4.2 HTTP path (`POST /v1/atm/heartbeat`)

```
hook process (sc-hooks) → JSON body: TeamMemberHeartbeatRequest
                                { team, member, pid, observed_at,
                                  activity, session_id? }
        │
        ▼
DaemonRequestDispatcher::record_heartbeat
        │ roster membership check, identity-conflict check
        ▼
RuntimeStatusCache::record_heartbeat(request, pid_changed)
        │ state transition (Active/Idle/Offline),
        │ non-overwrite merge for session_id,
        │ unconditional store for pid (heartbeat is authoritative)
        ▼
TeamMemberHeartbeatResponse { team, member, pid, pid_changed,
                              state, last_active_at, session_id }
```

**Note on heartbeat pid semantics:** heartbeat already treats `pid` as
*required* and *authoritative* — it is the basis for identity-conflict
detection (`runtime_health.rs:781`). That semantic is unchanged. The
non-overwrite rule applies only to fields that are *optional* on the wire
(`session_id` in heartbeat; both `session_id` and `pid` in UDS).

---

## 5. Sprint Decomposition

Total: **6 sprints**. Each is sized to land independently with green tests.
Dependencies are explicit; no sprint may start before its `Blocked-by` chain
is fully merged.

```
S1 (types) ──┐
             ├──► S3 (cache) ──► S5 (dispatcher) ──► S6 (snapshot+doctor)
S2 (wire)  ──┘         ▲
                       │
S4 (CLI caller ctx) ───┘
```

### Sprint 1 — `SessionId` type and protocol DTO extensions

**Blocked-by:** nothing (foundational).
**Blocks:** S2, S3, S4.

**Scope:**

- Add `SessionId` newtype to `crates/atm-core/src/types.rs`.
- Add `session_id: Option<SessionId>` to `TeamMemberHeartbeatRequest` and
  `TeamMemberHeartbeatResponse` in `crates/atm-core/src/protocol.rs`.
- Re-export `SessionId` from `crates/atm-core/src/lib.rs` alongside
  `AgentName`, `TeamName`, `ChatId`.
- Unit tests for the new type and the new serde behavior.

**Files:**

- `crates/atm-core/src/types.rs` (edit)
- `crates/atm-core/src/protocol.rs` (edit)
- `crates/atm-core/src/lib.rs` (edit — re-export)

**Tests:**

- `types::tests::session_id_newtype_round_trips_through_serde`
- `types::tests::session_id_display_matches_inner_string`
- `protocol::tests::heartbeat_request_omits_session_id_when_none`
- `protocol::tests::heartbeat_request_includes_session_id_when_some`
- `protocol::tests::heartbeat_response_omits_session_id_when_none`

**Acceptance criteria:**

- `cargo check -p atm-core` is clean.
- `cargo test -p atm-core --lib protocol` is green.
- Old JSON payloads (without `session_id`) still deserialize.
- New payloads with `session_id` round-trip identically.

---

### Sprint 2 — Wire payload extensions (`WriteRequest`, `ReadQuery`)

**Blocked-by:** S1.
**Blocks:** S5.

**Scope:**

- Add `session_id: Option<SessionId>` and `pid: Option<u32>` to
  `WriteRequest` (`crates/atm-core/src/send/mod.rs:69`) and to `ReadQuery`
  (`crates/atm-core/src/read/mod.rs:169`), both with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Update `WriteRequest::new()` and `ReadQuery::new()` to initialize both to
  `None` (existing call sites keep working unchanged; population from
  `CallerContext` happens in S5).
- Serde round-trip tests.

**Files:**

- `crates/atm-core/src/send/mod.rs` (edit)
- `crates/atm-core/src/read/mod.rs` (edit)

**Tests:**

- `send::tests::write_request_omits_session_id_and_pid_when_none`
- `send::tests::write_request_serializes_session_id_and_pid_when_some`
- `read::tests::read_query_omits_session_id_and_pid_when_none`
- `read::tests::read_query_serializes_session_id_and_pid_when_some`
- `read::tests::legacy_read_query_without_new_fields_deserializes`

**Acceptance criteria:**

- `cargo check -p atm-core` is clean.
- `cargo test -p atm-core --lib send` and `--lib read` are green.
- No call site changes required for existing constructors (verified by
  compile).

---

### Sprint 3 — `RuntimeStatusCache` storage and merge logic

**Blocked-by:** S1.
**Blocks:** S5, S6.

**Scope:**

- Extend `RuntimeMemberRecord` with `session_id: Option<SessionId>`
  (`crates/atm-daemon/src/runtime_status_cache.rs:27`).
- Add `RuntimeStatusCache::touch_member(team, member, session_id, pid,
  observed_at)` implementing the non-overwrite rule.
- Extend `record_heartbeat` to merge `session_id` (non-overwrite).
- Add `cached_session_id(team, member) -> Option<SessionId>`.
- Add unit tests covering all four quadrants of {member present, absent} ×
  {session_id Some, None}, plus the analogous pid quadrants.

**Files:**

- `crates/atm-daemon/src/runtime_status_cache.rs` (edit)

**Tests (all in `runtime_status_cache.rs`'s test module):**

- `touch_member_inserts_unknown_state_for_new_member`
- `touch_member_updates_last_active_at_for_existing_member`
- `touch_member_does_not_modify_state_field`
- `touch_member_overwrites_session_id_when_some`
- `touch_member_preserves_session_id_when_none`
- `touch_member_overwrites_pid_when_some`
- `touch_member_preserves_pid_when_none`
- `touch_member_respects_max_cache_entries_eviction`
- `record_heartbeat_merges_session_id_when_present`
- `record_heartbeat_preserves_session_id_when_absent`
- `cached_session_id_returns_none_for_unknown_member`

**Acceptance criteria:**

- `cargo test -p atm-daemon --lib runtime_status_cache` is green.
- All four {insert, update} × {Some, None} quadrants covered for both fields.
- No `is_some()` / `is_none()` inspection outside the merge function body.

---

### Sprint 4 — CLI caller context

**Blocked-by:** S1.
**Blocks:** S5.

**Scope:**

- Extend `CallerContext` with `session_id: Option<SessionId>` and
  `pid: Option<u32>` (`crates/atm-core/src/caller_context.rs:7`).
- Add `read_cli_session_id_from_env()` and `read_cli_pid_from_env()` with
  the `std::process::id()` fallback for the latter.
- Wire both into `resolve_cli_inspection_caller_context` and
  `resolve_cli_mutation_caller_context_with_overrides`.
- Update `crates/atm/src/commands/send.rs`, `read.rs`, `ack.rs` to copy
  `session_id` and `pid` from `CallerContext` into the request structs they
  build.

**Files:**

- `crates/atm-core/src/caller_context.rs` (edit)
- `crates/atm/src/commands/send.rs` (edit)
- `crates/atm/src/commands/read.rs` (edit)
- `crates/atm/src/commands/ack.rs` (edit)

**Tests:**

- `caller_context::tests::resolves_session_id_from_env_when_set`
- `caller_context::tests::session_id_is_none_when_env_unset`
- `caller_context::tests::resolves_pid_from_env_when_set`
- `caller_context::tests::pid_falls_back_to_process_id_when_env_unset`
- `caller_context::tests::rejects_malformed_pid_env_value`
- `caller_context::tests::rejects_non_utf8_session_id_env_value`
- Integration test: `cli_send_populates_session_id_and_pid_in_write_request`
  (uses a mock transport to capture the outgoing `WriteRequest`)

**Acceptance criteria:**

- `cargo test -p atm-core --lib caller_context` is green.
- `cargo test -p atm --test cli` (or equivalent) is green.
- With `ATM_SESSION_ID=foo ATM_PID=1234 atm send …`, the daemon sees both
  fields populated (verified by integration test, not by hand).

---

### Sprint 5 — Dispatcher integration (UDS + heartbeat)

**Blocked-by:** S2, S3, S4.
**Blocks:** S6.

**Scope:**

- `runtime_health.rs::route_write` — after successful
  `MessageWriter::write`, call `status_cache.touch_member(...)` with the
  request's caller identity, team, `session_id`, and `pid`
  (`runtime_health.rs:560-580`).
- `runtime_health.rs::dispatch_non_write` for `RequestEnvelope::Receive` —
  after successful `read_mail_with_runtime`, call
  `status_cache.touch_member(...)` from the `ReadQuery`
  (`runtime_health.rs:601-603`).
- `runtime_health.rs::record_heartbeat` — pass `request.session_id` through
  to `status_cache.record_heartbeat` (`runtime_health.rs:755-793`).
- **No new dispatch paths.** `Ack` already routes through `route_write`;
  heartbeat already routes through `record_heartbeat`.

**Files:**

- `crates/atm-daemon/src/runtime_health.rs` (edit)

**Tests:**

- `runtime_health::tests::route_write_touches_cache_on_success`
- `runtime_health::tests::route_write_does_not_touch_cache_on_validation_failure`
- `runtime_health::tests::receive_touches_cache_on_success`
- `runtime_health::tests::receive_does_not_touch_cache_on_error`
- `runtime_health::tests::ack_touches_cache_via_write_path`
- `runtime_health::tests::heartbeat_merges_session_id_into_cache`
- `runtime_health::tests::uds_then_heartbeat_merges_without_overwrite`
- `runtime_health::tests::heartbeat_then_uds_merges_without_overwrite`

**Acceptance criteria:**

- `cargo test -p atm-daemon --lib runtime_health` is green.
- Cache contents after each scenario verified by inspecting
  `RuntimeStatusCache::snapshot_for_members` in the test.
- No new branches on `Option::is_some()` for `session_id` or `pid` outside
  the cache module (verified by `rg "session_id\.is_(some|none)"
  crates/atm-daemon/src/` returning only `runtime_status_cache.rs` hits).

---

### Sprint 6 — Snapshot restructure, schema version bump, doctor/status output

**Blocked-by:** S3, S5.
**Blocks:** nothing (terminal sprint).

**Scope:**

- Restructure `RuntimeStatusSnapshot` to carry per-member records
  (`protocol.rs:405`). Replace `member_counts: RuntimeStatusCounts` with
  `members: Vec<RuntimeMemberStatus>` where:
  ```rust
  pub struct RuntimeMemberStatus {
      pub team: TeamName,
      pub member: AgentName,
      pub state: RuntimeMemberState,
      pub pid: Option<u32>,
      pub session_id: Option<SessionId>,
      pub last_active_at: Option<IsoTimestamp>,
  }
  ```
- Bump `CLI_SCHEMA_VERSION` from `1` to `2` (`protocol.rs:94`).
- Update `RuntimeStatusCache::snapshot` and `snapshot_for_members` to
  produce the new shape (`runtime_status_cache.rs:165-176`).
- Update `atm status` and `atm doctor` output
  (`crates/atm/src/output.rs:747` `print_runtime_status`) to render the new
  shape. Aggregated counts (active/idle/offline/unknown) are computed by the
  consumer from `members`, not received from the wire.

**Files:**

- `crates/atm-core/src/protocol.rs` (edit)
- `crates/atm-daemon/src/runtime_status_cache.rs` (edit)
- `crates/atm/src/output.rs` (edit)

**Tests:**

- `protocol::tests::runtime_status_snapshot_v2_serializes_per_member_records`
- `protocol::tests::cli_schema_version_is_two`
- `runtime_status_cache::tests::snapshot_includes_session_id_when_set`
- `runtime_status_cache::tests::snapshot_omits_session_id_when_none`
- `output::tests::print_runtime_status_renders_per_member_session_id`
- `output::tests::print_runtime_status_computes_counts_from_members`
- Compatibility test: a client built against schema v1 talking to a
  v2 daemon receives a clean version-incompatibility error.

**Acceptance criteria:**

- `cargo test --workspace` is green.
- `atm status` output includes a `session_id` column (or per-member line
  item) when at least one member has a session_id; omits the column when
  none do.
- `atm doctor` runtime section renders the new shape without panicking on
  either empty or populated caches.
- Old `RuntimeStatusCounts` struct is deleted (not just deprecated) — dead
  code is a planning bug.

---

## 6. Cross-Cutting Concerns

### 6.1 Testing strategy

- **Unit tests** live next to the code under test (workspace convention).
  Every sprint's "Tests" list names the test functions explicitly so the
  implementing agent does not have to invent names.
- **Integration tests** for the UDS path use the existing mock-transport
  harness in `atm-core` (pattern visible in existing `commands::send` tests).
- **End-to-end smoke**: after S6, run a real daemon on a scratch
  `ATM_HOME`, `atm send` a message with `ATM_SESSION_ID=test-sess
  ATM_PID=$$`, then `atm status` — confirm `test-sess` appears in the
  output. This is a manual verification step, not a CI test, because it
  requires a live daemon.

### 6.2 Error handling

- No new error variants. `session_id` / `pid` are observational; their
  absence or malformedness in *env vars* uses the existing
  `AtmError::caller_context_request_invalid` (same pattern as `ATM_CHAT_ID`
  in `caller_context.rs:194`).
- Daemon-side, a missing or unparsable `session_id` is **not** an error —
  the field is optional, so absence just means "no update". This is the
  non-overwrite rule applied at the boundary.

### 6.3 Logging and observability

- The daemon-side cache-touch should emit a `tracing::debug!` (not `info!`)
  with `subsystem="runtime_status_cache"`, `action="touch_member"`,
  `outcome="ok"`, plus `team`, `member`, and whether `session_id` / `pid`
  were present. Per the workspace advisory at the top of
  `.claude/skills/rust-development/guidelines.txt`, structured fields are
  mandatory.
- No `warn!` for absent fields — absence is normal, not a degradation.

### 6.4 Performance

- The cache touch is one `HashMap` lookup + occasional insert on an
  `ArcSwap`-protected clone-on-write structure. Cost is O(1) per UDS
  dispatch and trivially dominated by the actual mail I/O. No benchmarks
  required for this phase (per M-HOTPATH, only benchmark if a hot path is
  actually performance-relevant — this isn't one).
- `ArcSwap` clone-on-publish means readers never block. The new fields are
  small (`Option<String>` and `Option<u32>`); the per-publish allocation
  cost is unchanged in complexity class.

### 6.5 Security

- `session_id` is opaque to the daemon. It is never logged at `info!` or
  higher (PII-adjacent — a session_id can identify a specific user session).
  At `debug!` it is acceptable because `debug!` is not enabled in production
  builds by default.
- No authorization decisions are made on `session_id`. This is the
  behavioral-branching constraint restated at the security layer.

### 6.6 Backwards compatibility

- Wire: new fields are optional with `skip_serializing_if = "Option::is_none"`,
  so old binaries interoperate with new daemons and vice versa.
- Snapshot schema: broken intentionally (S6), mitigated by the
  `CLI_SCHEMA_VERSION` bump which forces a clean preflight failure rather
  than silent misparse.
- Env vars: `ATM_SESSION_ID` and `ATM_PID` are new. Their absence is the
  default and means "no update", not "error".

---

## 7. Build Sequence (Checklist)

Execute in order. Each step is a separate commit.

- [ ] **S1** — `SessionId` type + protocol DTO extensions
  - [ ] Add `SessionId` to `types.rs`
  - [ ] Add `session_id` to `TeamMemberHeartbeatRequest` / `Response`
  - [ ] Re-export from `lib.rs`
  - [ ] Unit tests green
  - [ ] Commit: `phase-aj(s1): add SessionId newtype and heartbeat DTO field`

- [ ] **S2** — Wire payload extensions
  - [ ] Extend `WriteRequest`
  - [ ] Extend `ReadQuery`
  - [ ] Serde round-trip tests green
  - [ ] Commit: `phase-aj(s2): thread session_id and pid through WriteRequest and ReadQuery`

- [ ] **S3** — Cache storage and merge
  - [ ] Extend `RuntimeMemberRecord`
  - [ ] Implement `touch_member` with non-overwrite rule
  - [ ] Extend `record_heartbeat` for `session_id`
  - [ ] Add `cached_session_id` accessor
  - [ ] All 11 quadrant tests green
  - [ ] Commit: `phase-aj(s3): add session_id storage and non-overwrite merge to RuntimeStatusCache`

- [ ] **S4** — CLI caller context
  - [ ] Extend `CallerContext`
  - [ ] Add env readers
  - [ ] Wire into resolvers
  - [ ] Update `send.rs`, `read.rs`, `ack.rs`
  - [ ] Integration test green
  - [ ] Commit: `phase-aj(s4): plumb session_id and pid through caller context and CLI commands`

- [ ] **S5** — Dispatcher integration
  - [ ] `route_write` cache touch
  - [ ] `Receive` cache touch
  - [ ] Heartbeat `session_id` pass-through
  - [ ] All 8 dispatcher tests green
  - [ ] Verify no out-of-module `session_id.is_some()` via grep
  - [ ] Commit: `phase-aj(s5): touch runtime status cache from UDS dispatch and heartbeat`

- [ ] **S6** — Snapshot restructure + version bump
  - [ ] Restructure `RuntimeStatusSnapshot`
  - [ ] Bump `CLI_SCHEMA_VERSION` to 2
  - [ ] Update `snapshot` / `snapshot_for_members`
  - [ ] Update `atm status` and `atm doctor` rendering
  - [ ] Delete `RuntimeStatusCounts`
  - [ ] `cargo test --workspace` green
  - [ ] Manual smoke: live daemon round-trip
  - [ ] Commit: `phase-aj(s6): restructure runtime snapshot to per-member records and bump schema`

---

## 8. Risks and Open Questions

| Risk | Likelihood | Mitigation |
|---|---|---|
| Snapshot restructure breaks a downstream consumer we haven't found (e.g. a script scraping `atm status --json`). | Medium | Grep the workspace for `RuntimeStatusSnapshot` and `member_counts` consumers during S6 *before* merging. The compatibility preflight will catch any consumer that goes through the daemon; raw JSON scrapers will not be caught — document the breaking change in the commit message and CHANGELOG. |
| `std::process::id()` fallback for `ATM_PID` reports the wrong pid when a hook script spawns a subshell that invokes `atm`. | Medium | Document the override mechanism (`ATM_PID=$PPID atm …`) in the env-var reference. The constraint "no behavioral branching on pid presence" means the daemon never *acts* on a wrong pid; only the cache value is wrong, which is observational. |
| `touch_member` on every `route_write` / `Receive` adds latency to hot paths. | Low | O(1) HashMap op under `ArcSwap`. If profiling ever shows it mattering, batch touches — but per M-HOTPATH we do not pre-optimize. |
| Reviewers accidentally add `if session_id.is_some()` branches later. | Medium | The S5 acceptance criterion includes a grep audit; add a CI lint (custom `clippy` restriction or `cargo-deny` rule) in a follow-up phase if the pattern recurs. Out of scope for Phase AJ itself. |

**Open questions to resolve during S1 review (not blocking, but flag):**

1. Should `SessionId` have a max length? Current decision: no, defer. If a
   hook ever sends a 1 MB session_id, the cache will hold it — but the
   eviction policy already caps the *number* of entries, not their size.
   Revisit only if a real incident occurs.
2. Should `RuntimeMemberStatus.last_active_at` advance on `touch_member`?
   Current decision: yes (§3.4). Counter-argument: it conflates "active"
   with "observed". Resolve in S3 review; the test names in §5.S3 assume
   "yes".

---

## 9. Beads Conversion Notes

When converting this plan to beads:

- One bead per sprint, titled `phase-aj/sN: <slug>` matching the commit
  messages in §7.
- Dependency edges exactly as drawn in §5: `s1→s2`, `s1→s3`, `s1→s4`,
  `s2→s5`, `s3→s5`, `s3→s6`, `s4→s5`, `s5→s6`.
- Each bead body should contain the full sprint section (everything under
  the `### Sprint N` heading) verbatim, so an agent picking up the bead via
  `br ready --json` has the complete spec without needing this file.
- Do **not** create per-file beads — that granularity is too fine and loses
  the "test what you build" cohesion within each sprint.

---

## 10. References

- Research document: `docs/plans/phase-aj/phase-aj-research.md` (same directory).
- Rust guidelines: `.claude/skills/rust-development/guidelines.txt` — in
  particular M-DESIGN-FOR-AI (newtype pattern, testability), M-CANONICAL-DOCS
  (every new public item gets canonical doc sections), and the
  `atm-daemon` structured-logging advisory.
- Existing patterns referenced inline: `AgentName` / `TeamName` / `ChatId`
  newtypes (`crates/atm-core/src/types.rs`), `CallerContext` env-var
  resolution (`crates/atm-core/src/caller_context.rs`),
  `RuntimeStatusCache` clone-on-write pattern
  (`crates/atm-daemon/src/runtime_status_cache.rs`),
  `DaemonRequestDispatcher` routing (`crates/atm-daemon/src/runtime_health.rs`).

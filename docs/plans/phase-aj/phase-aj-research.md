# Phase AJ Research: Agent State & Heartbeat Extension

**Date:** 2026-07-31  
**Branch:** `plan/phase-aj`  
**Status:** non-normative research. The authoritative baseline and contracts
are `plan-phase-aj.md` and the AJ sprint documents after the Phase AI cutover.

## Goal

Maintain runtime state for every roster member in the daemon's `RuntimeStatusCache`.
State is updated via two paths:

1. **CLI/graft commands** (send/read/ack) — extend existing local payload with
   optional environment-attested `ActivityObservation`. Daemon touches cache as
   a side effect of successful local dispatch.
2. **HTTP heartbeat endpoint** (`POST /v1/atm/heartbeat`) — extend
   `TeamMemberHeartbeatRequest` with `session_id`. Hook-driven.

Rule: absent optional fields do NOT overwrite existing cache values. A trusted
new pid/session becomes the current observation and emits retained diagnostic
evidence; it never changes routing, nudge, admission, retry, or lifecycle policy.

---

## Transport Architecture (to reverify at Phase AJ entry)

Both UDS and TCP transports use unified HTTP framing:

| Transport | File | Reader | Writer | Dispatcher |
|---|---|---|---|---|
| UDS | `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | `HttpFrameReader` | `write_local_http_response` | `ApiRouter` |
| TCP | `crates/atm-daemon/src/local_tcp_transport.rs` | `HttpFrameReader` | `write_local_http_response` | `ApiRouter` |

Supports keep-alive (`MAX_KEEP_ALIVE_REQUESTS`). On Unix, TCP runs as a secondary loopback HTTP listener for UDS/TCP parity. Both land in the same dispatcher; AJ.4 adds its one local-only `touch_member()` site there, so it covers both transports without transport-specific code.

### Files to Analyze / Modify

### 1. Protocol — Core Types
**File:** `crates/atm-core/src/protocol.rs`

| What | Change |
|---|---|
| `TeamMemberHeartbeatRequest` | Add `session_id: Option<SessionId>` |
| `TeamMemberHeartbeatResponse` | Add `session_id: Option<SessionId>` |
| `RuntimeMemberState` | AJ emits only Unknown, Offline, Idle, Active; `IdentityConflict` may remain only for compatibility deserialization |
| `RuntimeObservationSource` | Add `Heartbeat` and `LocalCommand` provenance values; neither is an authority or behavior selector |
| `RuntimeMemberObservation` | Add one public per-member snapshot DTO with state, optional session/pid, and state/session edge provenance |
| `RuntimeStatusSnapshot` | Add `members: Vec<RuntimeMemberObservation>` with current optional `session_id`, `pid`, provenance, and timestamps |
| `HeartbeatActivity` | No change (ActiveToolUse, Idle, SessionEnded) |

### 2. Wire Payload — WriteRequest
**File:** `crates/atm-core/src/send/mod.rs`

| What | Change |
|---|---|
| `ActivityObservation` | Add one nested transient DTO: team, member, optional session_id/pid; only environment-attested callers construct it |
| `WriteRequest` struct | Add `activity_observation: Option<ActivityObservation>` with additive serde; never persist it with mail |

### 3. Read Query
**File:** `crates/atm-core/src/read/mod.rs`

| What | Change |
|---|---|
| `ReadQuery` struct | Add `activity_observation: Option<ActivityObservation>`; never persist it with mail |

### 3a. Ack Conversion
**File:** `crates/atm-core/src/ack/mod.rs`

| What | Change |
|---|---|
| `AckRequest` | Add the same optional transient observation field |
| conversion | Preserve the field through `into_write_request()` and `from_unresolved_write()` so acknowledgement dispatch has the same observation semantics as send |

### 4. New Type — SessionId
**File:** `crates/atm-core/src/types.rs`

| What | Change |
|---|---|
| `SessionId` | Canonical `String` newtype; blank wire value normalizes to absent at cache merge |

### 5. Caller Context — Env Var Resolution
**File:** `crates/atm-core/src/caller_context.rs`

| What | Change |
|---|---|
| `CallerContext` struct | Add `activity_observation: Option<ActivityObservation>` |
| `read_cli_session_id_from_env()` | New function — reads `ATM_SESSION_ID` |
| `read_cli_pid_from_env()` | New function — reads `ATM_PID`; no process-ID fallback |
| `resolve_cli_*_caller_context()` | Construct observation only when environment team/identity attest resolved command identity |
| `activity_observation_for_resolved_caller()` | Shared non-fallible environment-attestation helper for CLI and graft; it parses raw `var_os` metadata locally, so absent/non-Unicode/malformed/mismatched telemetry is `None`, not a command error |

### 6. CLI Send Command
**File:** `crates/atm/src/commands/send.rs`

| What | Change |
|---|---|
| `SendCommand::build_request()` | Pass optional `activity_observation` from `CallerContext` into `WriteRequest` |

### 7. CLI Read Command
**File:** `crates/atm/src/commands/read.rs`

| What | Change |
|---|---|
| Read command | Pass optional `activity_observation` into `ReadQuery` |

### 8. CLI Ack Command
**File:** `crates/atm/src/commands/ack.rs`

| What | Change |
|---|---|
| Ack command | Pass optional `activity_observation` into `AckRequest`; the core acknowledgement conversion forwards it into the canonical write request |

### 9. Daemon Dispatch — Cache Touch
**File:** `crates/atm-daemon/src/runtime_health.rs`

| What | Change |
|---|---|
| `route_write()` | After successful local dispatch, touch cache with `activity_observation` only for `AuthenticatedIngress::Local` |
| `dispatch_non_write()` for `Receive` | Same — touch cache on successful local read only |
| `record_heartbeat()` | Accept and store `session_id` from heartbeat request |

### 10. Runtime Status Cache
**File:** `crates/atm-daemon/src/runtime_status_cache.rs`

| What | Change |
|---|---|
| Cache entry struct | Add current `session_id: Option<SessionId>`, `pid: Option<u32>`, provenance, and state-edge timestamp |
| `record_heartbeat()` | Update `session_id` only if `Some(...)`; required heartbeat pid replaces current pid |
| `merge_observation()` | One infallible cache merge helper; heartbeat and environment-attested local commands supply source/state, accepted-ingress order wins, `None` preserves |
| `ObservationMergeOutcome` | Crate-private comparison result reused for audit and `pid_changed` response construction |
| `touch_member()` | Thin local-command adapter around `merge_observation(ActivityObservation)` |
| `cached_session_id()` | New accessor |
| `snapshot()` | Include optional `session_id` and `pid` in `RuntimeStatusSnapshot` |

### 11. HTTP API Route
**File:** `crates/atm-core/src/api.rs`

| What | Change |
|---|---|
| `HEARTBEAT_PATH` | Already `/v1/atm/heartbeat` — no change; HTTPS peer ingress clears `activity_observation` from both Write and Receive request variants before shared dispatch |
| Serialization | `TeamMemberHeartbeatRequest` gains `session_id` — already derives Serialize/Deserialize |

### 12. Database Schema (NO CHANGE)
**File:** `crates/atm-storage-rusqlite/src/shared_db.rs`

State is in-memory only. No SQLite persistence for session_id, pid, or runtime
state, including no mail row/payload fields. The `team_roster` table already has
`member_kind` and `agent_type` — those are durable. Structured retained logs are
AJ's transition evidence; doctor aggregation and durable history are later work.

---

## Env Variables

| Variable | Purpose | CLI? | Hook? |
|---|---|---|---|
| `ATM_IDENTITY` | Agent identity | Required | Required |
| `ATM_TEAM` | Team name | Required | Required |
| `ATM_SESSION_ID` | Session identifier | Optional | Optional |
| `ATM_PID` | Process ID (or OS pid) | Optional | Required |

---

## Non-Overwrite Rule

When `session_id` is `None` in the incoming request:
- Do NOT clear the existing `session_id` in the cache
- Do NOT set it to `None`
- Leave the existing value untouched

When a local `ActivityObservation.pid` is `None`:
- Same rule — do not overwrite. Heartbeat pid is required and replaces current
  observation.

This applies to both the UDS dispatch path and the HTTP heartbeat path.

---

## Hard Constraint

> It is expressly forbidden for any assumptions to be made in software based on
> the presence/absence of these values.

No code may branch on session, pid, heartbeat activity, or derived state to
change behavior. The cache merge, audit, and snapshot projection are the only
allowed consumers; routing, nudge, notification, retry, admission, delivery,
and policy code are explicitly forbidden consumers.

# Phase AJ Research: Agent State & Heartbeat Extension

**Date:** 2026-07-31  
**Branch:** `plan/phase-aj`  
**Source:** atm-core `develop` at `a6cb4587`

## Goal

Maintain runtime state for every roster member in the daemon's `RuntimeStatusCache`.
State is updated via two paths:

1. **CLI commands** (send/read/ack) — extend existing UDS payload with optional
   `session_id` and `pid`. Daemon touches cache as a side effect of dispatch.
2. **HTTP heartbeat endpoint** (`POST /v1/atm/heartbeat`) — extend
   `TeamMemberHeartbeatRequest` with `session_id`. Hook-driven.

Rule: absent optional fields do NOT overwrite existing cache values. Only explicitly
provided values update state.

---

## Transport Architecture (integrate/phase-ai-31-33)

Both UDS and TCP transports use unified HTTP framing:

| Transport | File | Reader | Writer | Dispatcher |
|---|---|---|---|---|
| UDS | `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` | `HttpFrameReader` | `write_local_http_response` | `ApiRouter` |
| TCP | `crates/atm-daemon/src/local_tcp_transport.rs` | `HttpFrameReader` | `write_local_http_response` | `ApiRouter` |

Supports keep-alive (`MAX_KEEP_ALIVE_REQUESTS`). On Unix, TCP runs as a secondary loopback HTTP listener for UDS/TCP parity. Both land in the same dispatcher — `touch_member()` in dispatch handles all transports automatically.

### Files to Analyze / Modify

### 1. Protocol — Core Types
**File:** `crates/atm-core/src/protocol.rs`

| What | Change |
|---|---|
| `TeamMemberHeartbeatRequest` | Add `session_id: Option<SessionId>` |
| `TeamMemberHeartbeatResponse` | Add `session_id: Option<SessionId>` |
| `RuntimeMemberState` | No change (already covers Unknown, IdentityConflict, Offline, Idle, Active) |
| `RuntimeStatusSnapshot` | May need `session_id` per member |
| `HeartbeatActivity` | No change (ActiveToolUse, Idle, SessionEnded) |

### 2. Wire Payload — WriteRequest
**File:** `crates/atm-core/src/send/mod.rs`

| What | Change |
|---|---|
| `WriteRequest` struct | Add `session_id: Option<SessionId>`, `pid: Option<u32>` (both `#[serde(default, skip_serializing_if = "Option::is_none")]`) |

### 3. Read Query
**File:** `crates/atm-core/src/read/mod.rs`

| What | Change |
|---|---|
| `ReadQuery` struct | Add `session_id: Option<SessionId>`, `pid: Option<u32>` |

### 4. New Type — SessionId
**File:** `crates/atm-core/src/types/` (or new `crates/atm-core/src/session.rs`)

| What | Change |
|---|---|
| `SessionId` | New type alias or newtype wrapper (e.g., `String` or UUID) |

### 5. Caller Context — Env Var Resolution
**File:** `crates/atm-core/src/caller_context.rs`

| What | Change |
|---|---|
| `CallerContext` struct | Add `session_id: Option<SessionId>`, `pid: Option<u32>` |
| `read_cli_session_id_from_env()` | New function — reads `ATM_SESSION_ID` |
| `read_cli_pid_from_env()` | New function — reads `ATM_PID` (or `$$` default) |
| `resolve_cli_*_caller_context()` | Populate new fields from env |

### 6. CLI Send Command
**File:** `crates/atm/src/commands/send.rs`

| What | Change |
|---|---|
| `SendCommand::build_request()` | Pass `session_id` and `pid` from `CallerContext` into `SendRequest` |

### 7. CLI Read Command
**File:** `crates/atm/src/commands/read.rs`

| What | Change |
|---|---|
| Read command | Pass `session_id` and `pid` into `ReadQuery` |

### 8. CLI Ack Command
**File:** `crates/atm/src/commands/ack.rs`

| What | Change |
|---|---|
| Ack command | Pass `session_id` and `pid` into the write request |

### 9. Daemon Dispatch — Cache Touch
**File:** `crates/atm-daemon/src/runtime_health.rs`

| What | Change |
|---|---|
| `route_write()` | After dispatch, touch `RuntimeStatusCache` with caller identity + optional session_id/pid |
| `dispatch_non_write()` for `Receive` | Same — touch cache on read dispatch |
| `record_heartbeat()` | Accept and store `session_id` from heartbeat request |

### 10. Runtime Status Cache
**File:** `crates/atm-daemon/src/runtime_status_cache.rs`

| What | Change |
|---|---|
| Cache entry struct | Add `session_id: Option<SessionId>` |
| `record_heartbeat()` | Update `session_id` only if `Some(...)` |
| `touch_member()` | New method — update cache from write/read dispatch (same "don't overwrite None" rule) |
| `cached_session_id()` | New accessor |
| `snapshot()` | Include `session_id` in `RuntimeStatusSnapshot` |

### 11. HTTP API Route
**File:** `crates/atm-core/src/api.rs`

| What | Change |
|---|---|
| `HEARTBEAT_PATH` | Already `/v1/atm/heartbeat` — no change |
| Serialization | `TeamMemberHeartbeatRequest` gains `session_id` — already derives Serialize/Deserialize |

### 12. Database Schema (NO CHANGE)
**File:** `crates/atm-storage-rusqlite/src/shared_db.rs`

State is in-memory only. No SQLite persistence for session_id or runtime state. The
`team_roster` table already has `member_kind` and `agent_type` — those are durable.
Session state is ephemeral by design.

---

## Env Variables

| Variable | Purpose | CLI? | Hook? |
|---|---|---|---|
| `ATM_IDENTITY` | Agent identity | Required | Required |
| `ATM_TEAM` | Team name | Required | Required |
| `ATM_SESSION_ID` | Session identifier | Optional | Required |
| `ATM_PID` | Process ID (or OS pid) | Optional | Required |

---

## Non-Overwrite Rule

When `session_id` is `None` in the incoming request:
- Do NOT clear the existing `session_id` in the cache
- Do NOT set it to `None`
- Leave the existing value untouched

When `pid` is `None` in the incoming request:
- Same rule — do not overwrite

This applies to both the UDS dispatch path and the HTTP heartbeat path.

---

## Hard Constraint

> It is expressly forbidden for any assumptions to be made in software based on
> the presence/absence of these values.

No code may branch on "is session_id present?" to change behavior. The fields are
observational state only — they are recorded, not acted upon.
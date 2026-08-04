# Team Member State

This document is the authoritative contract for roster truth and the daemon's
runtime observation after Phase AJ.

> **Phase AJ implemented contract.** The rules and Rust shapes below describe
> the reconciled AJ implementation. AJ.9 must
> reconcile each clause with merged implementation source and named tests before
> this marker is removed.

## Ownership

- SQLite owns durable roster membership and routing metadata. It does not own
  live state, pid, session ID, timestamps, or observation history.
- `atm-daemon` owns one in-memory current observation keyed by `(team, member)`.
- Structured observability owns retained diagnostic events. AJ adds no database
  history, ring buffer, or doctor aggregation.

Durable roster changes never synthesize a runtime observation. Removing a
member drops its runtime entry; re-adding it starts without observation.
Updating roster metadata preserves any existing runtime entry.

## Current Observation

```rust
pub enum RuntimeMemberState {
    Unknown,
    Offline,
    Idle,
    Active,
    // IdentityConflict remains deserializable only for wire compatibility.
}

pub enum RuntimeObservationSource {
    Heartbeat,
    LocalCommand,
}

pub struct RuntimeMemberObservation {
    pub team: TeamName,
    pub member: AgentName,
    pub state: RuntimeMemberState,
    pub session_id: Option<SessionId>,
    pub pid: Option<u32>,
    pub last_active_at: Option<IsoTimestamp>,
    pub state_changed_by: Option<RuntimeObservationSource>,
    pub state_changed_at: Option<IsoTimestamp>,
    pub session_changed_by: Option<RuntimeObservationSource>,
    pub session_changed_at: Option<IsoTimestamp>,
}
```

`RuntimeMemberObservation` is a snapshot projection. The cache's internal
record may use a separate private type, but it has the same observation fields
and one crate-private, infallible merge owner.

## Closed Ingress Set

Only these accepted events can update current observation:

| Ingress | State | Metadata | Source |
| --- | --- | --- | --- |
| `POST /v1/atm/heartbeat` | `ActiveToolUse -> Active`, `Idle -> Idle`, `SessionEnded -> Offline` | required pid; optional session ID | `Heartbeat` |
| Successful environment-attested local `send`, `read`, or `ack` | `Active` | optional pid/session ID | `LocalCommand` |
| Graft read/send/ack with the same environment-derived caller context | `Active` | optional pid/session ID | `LocalCommand` |

Local request metadata uses one transient `ActivityObservation`:

```rust
pub struct ActivityObservation {
    pub team: TeamName,
    pub member: AgentName,
    pub session_id: Option<SessionId>,
    pub pid: Option<u32>,
}
```

It exists only when parseable `ATM_IDENTITY` and `ATM_TEAM` attest the resolved
caller. Arguments alone create no observation. An argument/environment mismatch
keeps the command's existing behavior, suppresses observation, and may emit an
info diagnostic. HTTPS peer ingress clears this transient field before shared
dispatch. The daemon accepts it only over existing authenticated local
UDS/loopback ingress; it never reads its own environment to infer provenance.
It never enters a mail row, message payload, or SQLite table.

Roster reload, daemon recovery, transport adapters, peer delivery, nudge,
notification, routing, retry, admission, and mailbox import are not ingress.

## Merge And Lifecycle Rules

- Merge order is accepted-ingress order, not client-clock order. AJ adds no
  stale-event rejection,
  timeout inference, PID liveness probe, or process-tree policy.
- `Some(session_id)` or `Some(pid)` replaces that field's current value. `None`
  and blank session IDs preserve the prior value. Heartbeat pid is required, so
  it always replaces the current pid.
- A changed pid/session is normal diagnostic evidence: retain it, audit it,
  and continue the ingress's lifecycle transition. Do not reject the request,
  set `IdentityConflict`, degrade readiness, alter eviction, or select a code
  path.
- Each actual initial or changed pid/session value emits one retained `info!`
  event with team, member, source, timestamp, and raw previous/new values.
  No-op or absent metadata emits no mutation event.
- `Unknown` means no trustworthy state observation. `Offline` means an
  explicit `SessionEnded` heartbeat. They are distinct and never inferred from
  timeout, missing data, a dead PID, roster data, or inbox state.
- A trusted local command moves the member to `Active`. A later heartbeat may
  move it to `Idle` or `Offline`. `state_changed_at` and its source update only
  on a real state edge; repeated evidence of the same state does not reset the
  edge time. `last_active_at` may advance on every trusted `Active` event.
- No ordinary reset API exists. Normal ingress cannot clear a known session or
  set a known state back to `Unknown`.

## Non-Authoritative Boundary

Session ID, PID, heartbeat activity, and derived state are telemetry only. No
routing, nudge, notification, retry, admission, delivery, access, or policy
decision may inspect them. Cache merge and snapshot projection are the only
allowed consumers. A future exception requires a named requirement, ADR,
boundary record, and regression test.

This deliberately permits a member's latest session/PID to toggle when two
legitimate checkouts or a rogue process share a member name. AJ records the
evidence; it does not attempt to decide which process is legitimate.

## Roster Projection

Structured output preserves raw session ID, pid, state, source, and absolute
timestamps. Human `atm members` output omits a default `Unknown` observation
with no session/pid. For a defined observation it may show state age, pid, and
the first 12 Unicode scalar values of session ID followed by `…` when longer.

## Required Tests

- heartbeat and environment-attested local/graft ingress are the only cache
  mutation paths;
- args-only and mismatched command identity produce no observation without
  changing existing command behavior;
- UDS, TCP, and heartbeat converge on one cache entry;
- absent/blank optional telemetry preserves known values;
- changed pid/session is logged and retained without conflict/rejection;
- `Unknown` and `Offline` remain distinct; only explicit heartbeat
  `SessionEnded` sets `Offline`;
- state-edge timestamps update only on a real transition;
- raw JSON and shortened human roster projections have the documented shape;
- a narrow source-use gate rejects observation references in policy modules.

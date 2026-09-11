# Team Member State

This document is the authoritative contract for durable roster truth and the
replacement Tokio/Axum runtime's canonical ephemeral agent state.

> **Phase AJ implemented contract.** The rules and Rust shapes below describe
> the AJ observation baseline as amended by issue #1378. ADR-045
> records the original evidence and the later canonical-state amendment.

## Ownership

- SQLite owns durable roster membership and routing metadata. It never owns
  live state, state revision, pid, session ID, timestamps, or observation
  history.
- The write-through RAM master roster owns one ephemeral state record for each
  durable `(team, member)`. `RosterRuntimeMirror` is the cross-crate read/write
  seam; the maintained `atm-http-runtime` composes and mutates it.
- `RuntimeHealth`, doctor, `teams`, and `members` are scoped projections of the
  master-roster record. They do not own another member-state map.
- Structured observability owns retained diagnostic events. No database
  history or second runtime cache is introduced.

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
    Blocked,
    // IdentityConflict remains deserializable only for wire compatibility.
}

pub enum RuntimeObservationSource {
    Heartbeat,
    HerdrPoll,
}

pub struct RosterMemberEphemeralState {
    pub state: RuntimeMemberState,
    pub session_id: Option<SessionId>,
    pub pid: Option<u32>,
    pub last_active_at: Option<IsoTimestamp>,
    pub availability: RuntimeObservationAvailability,
    pub last_observation_attempt_at: Option<IsoTimestamp>,
    pub last_observed_at: Option<IsoTimestamp>,
    pub revision: RosterStateRevision,
    pub last_observed_by: Option<RuntimeObservationSource>,
    pub state_changed_by: Option<RuntimeObservationSource>,
    pub state_changed_at: Option<IsoTimestamp>,
    pub session_changed_by: Option<RuntimeObservationSource>,
    pub session_changed_at: Option<IsoTimestamp>,
}
```

`RosterStateRevision` is a semantic newtype, not a raw cross-boundary integer.
It advances for every accepted state observation, even when the state value is
unchanged. `state_changed_at` advances only when the value changes.
The enclosing master-roster record supplies the typed `(TeamName, AgentName)`
identity; the ephemeral value does not duplicate it.
`RuntimeMemberObservation` is the read-only protocol projection of this record;
it is not a second owner.
`RuntimeObservationAvailability` is likewise typed (`Unobserved`, `Fresh`, or
`Unavailable`) and describes observation freshness, not a second lifecycle
state.

## Closed Ingress Set

Only these accepted events can update current observation:

| Ingress | State | Metadata | Source |
| --- | --- | --- | --- |
| `POST /v1/atm/heartbeat` | `ActiveToolUse -> Active`, `Idle -> Idle`, `SessionEnded -> Offline` | required pid; optional session ID | `Heartbeat` |
| Successful Herdr `agent list` poll | `working -> Active`, `idle/done -> Idle`, `blocked -> Blocked`, covered unknown/absent member -> Unknown | no pid/session mutation | `HerdrPoll` |

The pre-cutover `ActivityObservation` field remains tolerated as transient
wire-compatible request metadata and remote HTTPS ingress still strips it. It
does not update the canonical master-roster state and is not a
`RuntimeObservationSource`. This keeps older local clients non-fatal without
creating a third state authority while the legacy synchronous daemon awaits
Phase AM deletion.

Roster reload, daemon recovery, peer delivery, nudge emission, notification,
routing, retry, admission, and mailbox import are not state ingress. A failed
or incomplete Herdr list is also not ingress: it preserves the record and emits
the existing structured `ATM_HERDR_UNAVAILABLE` diagnostic while changing only
availability and `last_observation_attempt_at`. An observation for a member not
in the master roster fails with `ATM_MEMBER_NOT_FOUND`; ingress must not
synthesize durable membership.

## Merge And Lifecycle Rules

- Merge order is accepted-ingress order, not client-clock order. The merge
  owner advances `last_observed_at` and `RosterStateRevision` atomically with
  every accepted state observation. It adds no timeout inference, PID liveness
  probe, or process-tree policy.
- A successful Herdr result is one scoped batch mutation. Readers observe the
  roster before or after that batch, never a partially applied poll.
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
- `Unknown` means the latest successful authoritative observation for the
  covered member did not provide a known lifecycle value. `Offline` means an
  explicit `SessionEnded` heartbeat. They are distinct; timeout, poll failure,
  a dead PID, roster data, or inbox state never produces `Offline`.
- A heartbeat may move the member to `Active`, `Idle`, or `Offline` according
  to its explicit activity value. `state_changed_at` and its source update
  only on a real state edge; repeated evidence of the same state does not reset
  the edge time. `last_active_at` may advance on every accepted `Active` event.
- Normal ingress cannot clear a known session. A successful Herdr poll may set
  state to `Unknown` for a covered absent/unknown member without clearing that
  member's pid/session metadata.

## Nudge-policy exception

Session ID, PID, source, and timestamps are diagnostic metadata only. State
may drive exactly one policy, the Phase BA nudge invariant:

1. The nudge path MUST consume the exact canonical `RuntimeMemberState` from
   this record; it MUST NOT consume `PickerMemberStatus`, a `RuntimeHealth`
   projection, raw Herdr output, or heartbeat DTOs.
2. `Idle` with an open task MUST be nudged, no more than once per 60 seconds
   per task.
3. `Active` MUST never be nudged or diverted.
4. `Blocked` or `Offline` MUST escalate once per episode and MUST receive zero
   nudges.
5. Ingress handlers MUST update this record only and MUST NOT query work or
   emit a nudge.

No other routing, notification, retry, admission, delivery, access, or policy
decision may inspect runtime state or its metadata. A future exception requires
a named requirement, ADR, boundary record, and regression test.

This deliberately permits a member's latest session/PID to toggle when two
legitimate checkouts or a rogue process share a member name. AJ records the
evidence; it does not attempt to decide which process is legitimate.

## Roster Projection

Structured output preserves raw session ID, pid, exact state, availability,
revision, source, and absolute timestamps. Human `atm members` output may omit
only a default `Unknown`/`Unobserved` record with no session/pid. It must render
`Unavailable` freshness without changing the last exact state. For a defined
observation it may show state age, pid, and the first 12 Unicode scalar values
of session ID followed by `…` when longer.

## Required Tests

- heartbeat and successful Herdr poll observations converge on the same
  master-roster record;
- no second global member-state map exists in `RuntimeHealth` or a CLI path;
- legacy local activity metadata cannot create a canonical observation;
- absent/blank optional telemetry preserves known values;
- changed pid/session is logged and retained without conflict/rejection;
- a successful poll writes all covered members, including `Unknown` for
  absent/unknown values; a failed poll preserves state and revision;
- a failed poll changes only typed availability/attempt metadata and cannot
  trigger a nudge; a later accepted observation restores `Fresh`;
- a scoped Herdr poll applies as one batch rather than one roster clone per
  member;
- `Unknown` and `Offline` remain distinct; only explicit heartbeat
  `SessionEnded` sets `Offline`;
- state-edge timestamps update only on a real transition;
- every accepted state observation advances the typed revision;
- the nudge path consumes the exact canonical `RuntimeMemberState` and never
  raw Herdr output, a `RuntimeHealth` projection, or `PickerMemberStatus`;
- raw JSON and shortened human roster projections have the documented shape;
- a narrow source-use gate rejects observation references in policy modules.

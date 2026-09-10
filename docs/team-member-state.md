# Team Member State

This document is the authoritative contract for durable roster truth and the
replacement Tokio/Axum runtime's canonical ephemeral agent state.

> **Phase AJ implemented contract.** The rules and Rust shapes below describe
> the AJ observation baseline as amended by issue #1378 and Phase AZ. ADR-045
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
    LocalCommand,
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
- A trusted local command moves the member to `Active`. A later heartbeat may
  move it to `Idle` or `Offline`. `state_changed_at` and its source update only
  on a real state edge; repeated evidence of the same state does not reset the
  edge time. `last_active_at` may advance on every trusted `Active` event.
- Normal ingress cannot clear a known session. A successful Herdr poll may set
  state to `Unknown` for a covered absent/unknown member without clearing that
  member's pid/session metadata.

## Attention-policy exception

Session ID, PID, source, and timestamps are diagnostic metadata only. State may
drive exactly one policy: an accepted `Idle` observation revision publishes one
`IdleOpportunityId` to the Phase AZ attention selector. The ingress handler
does not query work or emit. The selector reserves zero or one item, then
revalidates that the same canonical member record is still `Idle` at the same
revision before emission. Replaying the same opportunity is idempotent.

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

- heartbeat, successful Herdr poll, and environment-attested local/graft
  activity converge on the same master-roster record;
- no second global member-state map exists in `RuntimeHealth` or a CLI path;
- args-only and mismatched command identity produce no observation without
  changing existing command behavior;
- UDS, TCP, and heartbeat converge on one cache entry;
- absent/blank optional telemetry preserves known values;
- changed pid/session is logged and retained without conflict/rejection;
- a successful poll writes all covered members, including `Unknown` for
  absent/unknown values; a failed poll preserves state and revision;
- a failed poll changes only typed availability/attempt metadata and cannot
  create an idle opportunity; a later accepted observation restores `Fresh`;
- a scoped Herdr poll applies as one batch rather than one roster clone per
  member;
- `Unknown` and `Offline` remain distinct; only explicit heartbeat
  `SessionEnded` sets `Offline`;
- state-edge timestamps update only on a real transition;
- every accepted state observation advances the typed revision and an `Idle`
  revision publishes exactly one idempotent opportunity;
- scheduling revalidates canonical state/revision and never consumes raw Herdr
  output or a `RuntimeHealth` projection;
- raw JSON and shortened human roster projections have the documented shape;
- a narrow source-use gate rejects observation references in policy modules.

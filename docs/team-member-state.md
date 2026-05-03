# Team Member State

This document is the authoritative Phase Q reference for how every team-member
field is populated and updated.

Goals:
- one auditable update path per field
- no fallback chains that silently rewrite ownership
- no ambiguous "best effort" state derivation
- minimal state machines only; no extra substates unless they remove a real
  ambiguity
- one place to answer:
  - how is `pid` set?
  - how is `last_active_at` set?
  - how is `state` set?
  - which component owns each update?

## Ownership Split

- SQLite owns durable roster truth.
- SQLite also owns the current durable `pid` for each team member.
- `atm-daemon` owns runtime liveness truth.
- hook/runtime layers report runtime facts to `atm-daemon`; they do not become
  the source of truth themselves.

## Durable Roster Fields

```rust
pub struct TeamMateRecord {
    pub team: TeamName,
    pub name: AgentName,
    pub provider: ProviderName,
    pub model: ModelName,
    pub transport_kind: Option<TransportKind>,
    pub host_name: Option<HostName>,
    pub tmux: Option<TmuxLocator>,
    pub recipient_pane_id: Option<PaneId>,
    pub pid: Option<u32>,
    pub metadata_json: Option<String>,
}

pub struct TmuxLocator {
    pub session_name: Option<String>,
    pub window_name: Option<String>,
    pub pane_id: Option<PaneId>,
}
```

### Durable Field Update Rules

| Field | Owner | Allowed update paths | Forbidden update paths |
|---|---|---|---|
| `team` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | hook events; daemon liveness probes |
| `name` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | hook events; daemon liveness probes |
| `provider` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | inferred from pid; inferred from recent activity |
| `model` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | inferred from pid; inferred from recent activity |
| `transport_kind` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | transport handshake side effects |
| `host_name` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | inferred from socket peer state |
| `tmux.session_name` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | hook events; runtime liveness logic |
| `tmux.window_name` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | hook events; runtime liveness logic |
| `tmux.pane_id` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | hook events; runtime liveness logic |
| `recipient_pane_id` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | post-send hook side effects; runtime liveness logic |
| `pid` | SQLite roster store | documented heartbeat handler writes only | ad hoc runtime cache writes; roster ingest guesses |
| `metadata_json` | SQLite roster store | roster bootstrap; config/roster ingest; explicit roster repair/admin path | runtime liveness logic |

Rule:
- runtime activity must not mutate durable roster fields unless the update
  comes through one explicit roster-write path.

## Runtime State Fields

```rust
pub struct TeamMateRuntime {
    pub last_active_at: Option<DateTime<Utc>>,
    pub state: AgentState,
}

pub enum AgentState {
    Unknown,
    Offline,
    Idle,
    Active,
}

pub struct TeamMateView {
    pub team: TeamName,
    pub name: AgentName,
    pub provider: ProviderName,
    pub model: ModelName,
    pub transport_kind: Option<TransportKind>,
    pub host_name: Option<HostName>,
    pub tmux: Option<TmuxLocator>,
    pub recipient_pane_id: Option<PaneId>,
    pub pid: Option<u32>,
    pub last_active_at: Option<DateTime<Utc>>,
    pub state: AgentState,
}
```

## Runtime Field Update Rules

All daemon-managed team-member fields must update through one daemon socket
handler:

```rust
pub struct TeamMateHeartbeat {
    pub team: TeamName,
    pub name: AgentName,
    pub pid: u32,
    pub observed_at: DateTime<Utc>,
    pub activity: HeartbeatActivity,
}

pub enum HeartbeatActivity {
    ActiveToolUse,
    Idle,
    SessionEnded,
}
```

Allowed producers:
- ATM CLI
- hook/runtime layer

Forbidden producers:
- direct SQLite writes
- ad hoc liveness polling that bypasses the socket handler
- transport adapters performing inline state mutation outside the heartbeat
  handler

## Required State Machines

These state machines are the complete Phase Q transition model for team-member
runtime state. No additional state machines or hidden fallback transitions are
permitted unless this document is updated first.

Implementation rule:
- per `RBP-002 Typestate`, illegal transitions should be made impossible at
  compile time where practical
- at minimum, transition logic must live in one closed module with one explicit
  transition API rather than being reimplemented across handlers

### Runtime Activity State Machine

States:
- `Unknown`
- `Active`
- `Idle`
- `Offline`

Allowed transitions:

```text
Unknown --heartbeat(active)--> Active
Unknown --heartbeat(idle)----> Idle
Unknown --session-ended------> Offline

Active --heartbeat(active)---> Active
Active --heartbeat(idle)-----> Idle
Active --session-ended-------> Offline
Active --pid-dead-----------> Offline

Idle ----heartbeat(active)---> Active
Idle ----heartbeat(idle)-----> Idle
Idle ----session-ended-------> Offline
Idle ----pid-dead-----------> Offline

Offline -heartbeat(active)---> Active
Offline -heartbeat(idle)-----> Idle
Offline -session-ended-------> Offline
```

Forbidden transitions:

```text
Unknown -> Offline by timeout/guess alone
Any -> Active without a documented heartbeat
Any -> Idle without a documented heartbeat
Any -> Offline from roster-only or inbox-only inference
```

### PID Ownership State Machine

States:
- `UnregisteredPid`
- `RegisteredPid(pid)`
- `Conflict(old_pid, new_pid)`

Allowed transitions:

```text
UnregisteredPid --heartbeat(pid=P)----------------------> RegisteredPid(P)
RegisteredPid(P) --heartbeat(pid=P)---------------------> RegisteredPid(P)
RegisteredPid(P_old) --heartbeat(pid=P_new), old dead --> RegisteredPid(P_new) + AgentPidChanged
RegisteredPid(P_old) --heartbeat(pid=P_new), old live --> Conflict(P_old, P_new)
Conflict(P_old, P_new) --admin-assume-identity---------> RegisteredPid(P_new) + AgentPidChanged
```

Forbidden transitions:

```text
RegisteredPid(P_old) --heartbeat(pid=P_new), old live --> RegisteredPid(P_new)
Conflict(...) -> RegisteredPid(...) without explicit admin path
Any pid change by roster ingest or direct SQLite write
```

### Identity Conflict State Machine

States:
- `NoConflict`
- `IdentityConflict`

Allowed transitions:

```text
NoConflict --live-old-pid + new-pid heartbeat--> IdentityConflict
IdentityConflict --admin-assume-identity-------> NoConflict
IdentityConflict --old-pid dead + retry--------> NoConflict
```

Required behavior in `IdentityConflict`:
- reject the calling heartbeat/CLI action
- return `ATM_IDENTITY_CONFLICT`
- return the exact caller-facing stop message:
  - `ATM_IDENTITY_CONFLICT: stop and report to user immediately`
- do not overwrite durable pid
- do not mutate runtime `state` to accommodate the new pid

Admin-only exit path:
- explicit admin command such as
  `atm admin assume-identity --team <team> --agent <name> --pid <new_pid>`
- this command may mark the old pid/session as rogue or superseded
- this command must update durable pid, emit `AgentPidChanged`, and emit an
  audit event

### `pid`

Owner:
- SQLite durable roster/runtime identity field
- cached by `atm-daemon` for runtime liveness checks

Authoritative update path:
1. `TeamMateHeartbeat` accepted by `atm-daemon`.
   - Claude-compatible hook producer supplies the stable parent session pid
   - Codex/native producer supplies the agent process pid itself

Forbidden update paths:
- inferred from roster file
- inferred from tmux pane only
- inferred from previous cached pid without a fresh confirming event

Rules:
- `pid` is the primary liveness field
- replacing `pid` is allowed only through the documented heartbeat handler,
  which updates SQLite and then refreshes daemon runtime state
- if a new event reports a different pid for the same member and the old pid is
  dead, the daemon updates SQLite, refreshes runtime state, and emits
  `AgentPidChanged`
- if a new event reports a different pid for the same member and the old pid is
  still alive, the daemon must reject the new heartbeat as an identity conflict
  unless the explicit admin takeover path below is active
- a live-old-pid plus new-pid conflict is a security event, not a normal
  respawn path

### `last_active_at`

Owner:
- `atm-daemon` runtime state

Authoritative update path:
1. `TeamMateHeartbeat { activity: ActiveToolUse | Idle | SessionEnded }`
   accepted by `atm-daemon`

Forbidden update paths:
- background polling with no attributable agent event
- read-only roster inspection
- mailbox import of unrelated inbound messages

Rules:
- update only when a concrete attributable activity event occurs
- do not synthesize activity timestamps from daemon startup time
- do not backfill from durable roster metadata

### `state`

Owner:
- `atm-daemon` runtime state

Authoritative update paths:
1. `Unknown`
   - daemon startup before the member has emitted any authoritative runtime
     event in the current daemon lifetime
2. `Active`
   - `TeamMateHeartbeat { activity: ActiveToolUse }`
3. `Idle`
   - `TeamMateHeartbeat { activity: Idle }`
4. `Offline`
   - liveness check proves tracked `pid` is dead
   - `TeamMateHeartbeat { activity: SessionEnded }`

Forbidden update paths:
- inferred from roster-only data
- inferred from "has inbox messages"
- inferred from stale `last_active_at` without the documented idle/offline rule

Rules:
- `state` changes only from one of the four update classes above
- no undocumented fallback chain may map missing data directly to `Offline`
- missing runtime data yields `Unknown`, not `Offline`

## PID Capture Contract

### Claude-Compatible Sessions

- the hook/runtime layer must capture the stable parent session pid
- the hook subprocess pid is never valid as the member `pid`
- the hook/runtime layer must send that pid to the daemon through the
  `TeamMateHeartbeat` socket path

Current interim source:
- the already-installed Python hooks from `../agent-team-mail`

Future source:
- `schooks 1.0`

### Codex Sessions

- Codex does not rely on Claude hook semantics
- the ATM CLI/runtime must send the Codex process pid to the daemon through the
  same `TeamMateHeartbeat` socket path
- that pid is the authoritative runtime pid until superseded by a new explicit
  heartbeat/session event

## No Fallback-Soup Rule

The following are prohibited:
- multi-step undocumented fallback chains for `state`
- multi-step undocumented fallback chains for `pid`
- mutating roster settings as a side effect of liveness tracking
- mutating runtime liveness fields as a side effect of roster ingest
- silent ownership transfer between store, daemon, and hook layers
- adding a second runtime state mutation path besides the heartbeat socket
  handler without updating this document

If a field has more than one allowed update path, every path must be listed in
this document.

## Required Tests

- roster field writes occur only through documented roster-write paths
- runtime events update `pid` only from documented authoritative sources
- pid changes with a still-live old pid are rejected unless an explicit
  admin takeover path is active
- runtime events update `last_active_at` only from attributable activity events
- both CLI-originated and hook-originated activity go through the same
  heartbeat socket handler
- state transitions are limited to the documented `Unknown/Offline/Idle/Active`
  transition causes
- a dead tracked pid transitions the member to `Offline`
- missing runtime data leaves the member `Unknown`, not `Offline`
- daemon restart resets runtime-only state to `Unknown` until fresh events
- projected `TeamMateView` combines durable and runtime fields without
  cross-layer mutation

# OBS-GAP-1: Historical `sc-observability` API Gap Analysis For ATM

## Status Note

This document is historical.

The shared `sc-observability` repo now ships the generic query, follow, and
query-health surfaces that were missing when this analysis was first written.
The active ATM-side work is no longer "request the missing shared API"; it is
"integrate ATM with the current shared API".

Current controlling ATM integration documents:

- [`../project-plan.md`](../project-plan.md) Phase K
- [`../requirements.md`](../requirements.md) §3.5 and §13
- [`../architecture.md`](../architecture.md) §2.3 and §14
- [`../atm-core/design/sc-observability-integration.md`](../atm-core/design/sc-observability-integration.md)

## 1. Purpose

This document closes Phase A sprint `OBS-GAP-1` for `atm-core`.

Its purpose is to verify whether the current shared `sc-observability` workspace
already provides the capability surface required by the retained ATM commands:

- `atm send`
- `atm read`
- `atm ack`
- `atm clear`
- `atm log`
- `atm doctor`

The focus is the observability boundary only. This is a planning artifact, not
an implementation plan for Rust code inside `atm-core`.

## 2. ATM-Side Required Capability List

ATM needs four observability-facing responsibilities at the `atm-core`
boundary:

1. best-effort command/event emission for normal mail commands
2. historical log query for `atm log`
3. follow/tail of new matching log records for `atm log --tail`
4. health/readiness inspection for `atm doctor`

The retained ATM architecture already fixes the ownership split:

- `atm-core` owns the sealed `ObservabilityPort`
- `atm-core` owns ATM-specific event/query vocabulary
- `atm` owns the concrete shared-crate integration
- shared `sc-observability` should own generic log storage/query/follow/filter
  behavior

### 2.1 Required `ObservabilityPort` Capabilities

The ATM-owned port needs to support the following operations:

| Port responsibility | Used by | Requirement |
| --- | --- | --- |
| emit ATM command lifecycle records | `send`, `read`, `ack`, `clear` | best-effort, must not break core command correctness |
| query retained log records | `log` | historical record access with filters and ordering |
| follow new matching log records | `log --tail` | long-lived follow mode over the shared log store |
| read logging health | `doctor` | in-process health snapshot and active log-path visibility |
| check query readiness | `doctor` | verify the query/follow surface is usable for `atm log` |

### 2.2 `atm log` Required Capabilities

`atm log` needs the following shared behavior:

- read retained structured records
- follow new matching records
- filter by severity level
- filter by structured ATM fields such as `command`, `team`, `actor`, `target`,
  `task_id`, and `outcome`
- filter by time window
- apply limit and ordering controls
- render either human output or JSON from the ATM side without re-parsing raw
  daemon-era files

ATM-specific ownership for `atm log` should remain limited to:

- ATM default filters
- ATM field names and field projection
- CLI rendering and JSON output shape

### 2.3 `atm doctor` Required Capabilities

`atm doctor` needs the following shared behavior:

- initialize observability at process startup
- expose in-process health state
- report active log path / log-root resolution
- expose sink/runtime degradation information
- provide a queryable surface that can be probed so `doctor` can report whether
  `atm log` is operational

ATM-specific ownership for `atm doctor` should remain limited to:

- command-local findings and severity grading
- ATM config/env/path checks
- ATM-specific recovery messaging

## 3. Gap List Against Current `sc-observability`

This section compares ATM requirements with the current shared repo at:

- `/Users/randlee/Documents/github/sc-observability/crates/sc-observability`
- `/Users/randlee/Documents/github/sc-observability/crates/sc-observe`
- `/Users/randlee/Documents/github/sc-observability/crates/sc-observability-types`

### 3.1 Capability Matrix

| Required capability | Status | Current evidence | Gap summary |
| --- | --- | --- | --- |
| Structured log emission | Present | `sc-observability::Logger::emit`, `LogEmitter`, `LogEvent` | Shared logging emission already exists and matches ATM best-effort needs. |
| In-process logging health | Present | `Logger::health`, `LoggingHealthReport`, `active_log_path` | Sufficient baseline for `atm doctor` health reporting. |
| Top-level routing health | Present | `sc-observe::Observability::health`, `ObservabilityHealthReport` | Available if ATM later adopts typed routing; not required for MVP. |
| Historical retained-log query | Missing | no public query/read API in `sc-observability` or `sc-observe` | ATM cannot implement `atm log` without building its own reader unless shared API is added. |
| Follow/tail of new matching records | Missing | no public follow/tail API in `sc-observability` or `sc-observe` | ATM cannot implement `atm log --tail` on shared infra today. |
| Severity filter on consumer path | Partial | `Level`, `LevelFilter`, `LogEvent.level` exist | The schema supports level filtering, but only emit/config surfaces exist; there is no shared query API to apply it on read. |
| Structured field filtering on consumer path | Partial | `LogEvent.fields` exists | Structured fields exist, but there is no shared query/follow filter surface. |
| Time-window filtering | Partial | `LogEvent.timestamp` exists | Timestamps exist, but there is no query surface for `since` / `until` filtering. |
| Limit and ordering controls | Missing | no query result model | ATM has no shared way to request newest-first snapshots or capped result sets. |
| Query readiness probe for doctor | Missing | health exists, query surface does not | `atm doctor` cannot verify `atm log` readiness until shared query/follow APIs exist. |

### 3.2 Interpretation

The current shared workspace is already good enough for:

- ATM best-effort emission
- ATM logging health inspection
- future routing health, if ATM later chooses to emit typed observations

The current shared workspace is not yet good enough for:

- `atm log`
- `atm log --tail`
- the `atm doctor` check that verifies log-query readiness

The missing capability is not generic logging itself. The missing capability is
consumer-side access to the retained structured log records.

## 4. Concrete API Requests For `arch-obs`

ATM should ask `arch-obs` to add a shared log-reader/query/follow surface to
`sc-observability`.

This should live in `sc-observability`, not `sc-observe`, because the required
behavior is part of the logging-only layer:

- it operates on persisted `LogEvent` records
- it should work for a basic CLI without the typed routing runtime
- the shared repo architecture already says generic log query/follow behavior
  belongs in the shared logging layer when possible

### 4.1 Request 1: Public Log Query Model

Add public neutral query types to `sc-observability` (or
`sc-observability-types` if shared across crates) for retained-log access.

Recommended minimum shape:

```rust
pub struct LogQuery {
    pub service: Option<ServiceName>,
    pub levels: Vec<Level>,
    pub field_matches: Vec<FieldMatch>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub limit: Option<usize>,
    pub order: LogOrder,
}

pub struct FieldMatch {
    pub key: String,
    pub value: serde_json::Value,
}

pub enum LogOrder {
    NewestFirst,
    OldestFirst,
}
```

Required behavior:

- service scoping
- level filtering
- exact structured field matching
- time-window filtering
- limit and ordering

V1 does not need regex, fuzzy matching, or advanced predicates.

### 4.2 Request 2: Public Historical Query API

Add a public shared query surface in `sc-observability` for reading retained
records.

Recommended minimum shape:

```rust
pub struct LogSnapshot {
    pub records: Vec<LogEvent>,
    pub truncated: bool,
}

impl Logger {
    pub fn query(&self, query: &LogQuery) -> Result<LogSnapshot, QueryError>;
}
```

Equivalent alternatives are acceptable if they preserve the same behavior.

Required behavior:

- read from the shared JSONL log store
- return structured `LogEvent` values, not raw lines
- treat malformed/unreadable lines as typed query errors or skipped-record
  accounting, not silent ATM-owned parsing work

### 4.3 Request 3: Public Follow/Tail API

Add a public follow surface for new matching records.

Recommended minimum shape:

```rust
pub struct LogFollowOptions {
    pub query: LogQuery,
    pub from_end: bool,
}

pub struct LogFollowSession { /* opaque */ }

impl Logger {
    pub fn follow(&self, options: LogFollowOptions) -> Result<LogFollowSession, QueryError>;
}

impl LogFollowSession {
    pub fn next(&mut self) -> Result<Option<LogEvent>, QueryError>;
}
```

Required behavior:

- filter new records using the same query semantics as historical reads
- support tailing from the live end of the file
- expose an owning session type rather than a callback-only API

ATM does not need shared async streams in v1. A blocking iterator/session model
is sufficient.

### 4.4 Request 4: Query/Follow Error Surface

Add a typed shared error for query/follow operations.

Recommended minimum shape:

- `QueryError` with structured diagnostic context
- clear distinction between:
  - invalid query input
  - unreadable/missing log store
  - malformed record decoding
  - follow-session shutdown/unavailable state

This is needed so `atm doctor` can convert shared query failures into stable
diagnostic findings.

### 4.5 Request 5: Query Health / Readiness Signal

Either of the following is acceptable:

1. extend `LoggingHealthReport` with explicit query-readiness information, or
2. guarantee that `Logger::query(...limit=1...)` is the supported readiness
   probe for the log reader surface

ATM does not need a second health subsystem. It needs a reliable way for
`atm doctor` to answer:

- is logging initialized?
- where is the active log file?
- is the shared log query surface operational?

### 4.6 Request 6: File-Scope Reader Independence

Ensure the shared query/follow API works for logging-only consumers that use
`sc-observability` directly, without forcing adoption of `sc-observe` or OTLP.

This keeps the query/follow surface aligned with the documented layered design.

## 5. ATM Port Boundary Decision

The boundary decision is:

- `atm-core::ObservabilityPort` stays ATM-owned
- shared `sc-observability` owns generic emission, storage, query, follow, and
  health mechanics
- `atm` implements the ATM port by translating ATM query/event models into the
  shared `sc-observability` API

### 5.1 ATM-Owned Responsibilities

The ATM port should continue to own:

- ATM command lifecycle event names
- ATM field names and ATM default query presets
- CLI-facing `atm log` render/output behavior
- CLI-facing `atm doctor` finding/report behavior
- best-effort policy for mail-command emission

### 5.2 Shared Responsibilities

Shared `sc-observability` should own:

- JSONL log writing
- retained-record decoding
- generic historical query
- generic follow/tail mechanics
- generic level/field/time filtering
- generic limit/order behavior
- in-process logging health snapshots

### 5.3 Shared Responsibilities That ATM Does Not Need Yet

ATM does not need the following to start Phase A/B implementation:

- OTLP export
- typed observation routing through `sc-observe`
- ATM-specific envelope compatibility behavior
- daemon-era spool/fan-in behavior

Those remain separate concerns.

## 6. Conclusion

### 6.1 Shared-Crate Readiness

Current shared-crate readiness is:

- sufficient for ATM best-effort log emission
- sufficient for ATM in-process logging health
- not yet sufficient for `atm log`
- not yet sufficient for the `atm doctor` check that validates `atm log`
  readiness

### 6.2 ATM-Local Query Engine Decision

ATM should **not** build a local ad hoc log query engine.

Reason:

- the missing behavior is generic shared logging functionality, not ATM-specific
  product logic
- building an ATM-only JSONL reader/tailer would duplicate the shared logging
  layer and create long-term ownership drift
- the retained ATM architecture already expects generic query/follow mechanics
  to live in the shared observability layer

Therefore the required path is:

1. `arch-obs` adds the shared query/follow/readiness API
2. `atm` implements `ObservabilityPort` against that shared API
3. `atm-core` keeps only ATM-specific event/query vocabulary and command logic

### 6.3 Phase A Exit Condition

`OBS-GAP-1` is complete once the shared API request is explicit and the ATM
boundary decision is fixed.

`OBS-GAP-1` does **not** require ATM to implement observability code yet. It
requires the shared-observability blocker to be clearly defined so Phase B/C
implementation can proceed without ATM inventing its own log-reader stack.

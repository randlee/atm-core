# `sc-observability` Integration Plan

## 1. Purpose

This document defines the production implementation plan for integrating
`atm-core` and `atm` with the shared `sc-observability` workspace.

It replaces the older assumption that ATM is still waiting on a missing shared
query/follow surface. That gap is closed in the current shared repo. The
remaining work is ATM-side integration, projection, testing, and rollout.

## 2. Current State

Current ATM runtime state:

- `atm-core::observability::ObservabilityPort` only supports
  `emit_command_event(...)`
- `atm` implements that emit path with local `tracing`
- `atm log` is not implemented
- `atm doctor` is still a stub

Current shared `sc-observability` state:

- `sc-observability-types` owns `LogQuery`, `LogSnapshot`, `QueryError`,
  `QueryHealthReport`, and `LoggingHealthReport`
- `sc-observability` owns `Logger`, `LoggerConfig`, `JsonlFileSink`,
  `ConsoleSink`, `JsonlLogReader`, `Logger::query(...)`,
  `Logger::follow(...)`, and `Logger::health()`
- the shared logging layer already provides the generic file-backed
  query/follow/filter/readiness surface ATM needs for retained `log` and
  `doctor`

This means ATM no longer needs an API-gap phase. It needs an implementation
phase.

## 3. Ownership Split

Shared `sc-observability` owns:

- structured log emission
- file and console sinks
- JSONL retention, rotation, and active-log-path layout
- synchronous historical query
- synchronous follow/tail
- query health and logging health reports

`atm-core` owns:

- the sealed/injected observability boundary consumed by ATM services
- ATM-owned command-event vocabulary
- ATM-owned log query defaults and field projections
- ATM-owned doctor findings projected from shared health data
- structured ATM error mapping above shared query and health failures

`atm` owns:

- concrete dependency wiring to `sc-observability`
- startup initialization of the shared logger
- env/config translation for ATM-specific log-root and sink policy
- injection of the concrete observability adapter into `atm-core`

`atm-core` must not import `sc-observability` directly.

## 4. Initial Integration Scope

The initial retained-command integration scope is:

- `sc-observability-types`
- `sc-observability`

The initial integration does not require:

- `sc-observe`
- `sc-observability-otlp`

Those higher layers remain available for future ATM telemetry or typed-routing
work, but they are not required to deliver retained `send`, `read`, `ack`,
`clear`, `log`, and `doctor`.

## 5. Pre-Publish Dependency Strategy

Until `sc-observability` is published, ATM integration may consume the shared
crates from a local checkout in both developer builds and CI.

Required rules:

- the committed ATM design must target the real shared crate names and APIs
- no ATM code may hardcode a user-specific absolute checkout path
- local and CI builds may use a repo-local Cargo patch/path strategy that
  points to a checked-out sibling `sc-observability` repo
- the same dependency strategy must work in CI so production-readiness testing
  is not a developer-only path
- once `sc-observability` is published, ATM should switch to versioned crate
  dependencies with minimal code churn

Operational detail for the pre-publish period is documented in:

- [`../dev/pre-publish-deps.md`](../dev/pre-publish-deps.md)

Toolchain rule:

- this phase assumes the shared-repo toolchain floor is adopted across ATM and
  `sc-*` repos
- the active target is Rust `1.94.1`
- the same pinned toolchain must be used locally and in CI

## 6. Required `ObservabilityPort` Expansion

The current emit-only boundary is insufficient. The retained boundary must grow
to cover emission, query, follow, and health.

Required ATM-owned projected types:

```rust
pub struct AtmLogQuery {
    pub levels: Vec<LogLevelFilter>,
    pub field_matches: Vec<LogFieldMatch>,
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
    pub limit: Option<usize>,
    pub order: LogOrder,
}

pub struct AtmLogRecord {
    pub timestamp: IsoTimestamp,
    pub severity: LogLevelFilter,
    pub service: String,
    pub target: Option<String>,
    pub action: Option<String>,
    pub message: Option<String>,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

pub struct AtmLogSnapshot {
    pub records: Vec<AtmLogRecord>,
    pub truncated: bool,
}

pub enum AtmLoggingHealthState {
    Healthy,
    Degraded,
    Unavailable,
}

pub enum AtmQueryHealthState {
    Healthy,
    Degraded,
    Unavailable,
}

pub struct ObservabilityHealthSnapshot {
    pub active_log_path: AbsolutePath,
    pub logging_state: AtmLoggingHealthState,
    pub query_state: Option<AtmQueryHealthState>,
    pub last_error: Option<String>,
}
```

Required ATM-owned error-code type:

```rust
pub enum AtmErrorCode {
    // centrally defined in crates/atm-core/src/error_codes.rs
}
```

Required private/object-safe session boundary:

```rust
trait LogFollowPort: Send {
    fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError>;
    fn query_health(&self) -> Result<ObservabilityHealthSnapshot, AtmError>;
}

pub struct LogTailSession {
    inner: Box<dyn LogFollowPort>,
}
```

Required injected boundary:

```rust
pub trait ObservabilityPort {
    fn emit_command_event(&self, event: CommandEvent) -> Result<(), AtmError>;
    fn query_logs(&self, query: &AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>;
    fn follow_logs(&self, query: AtmLogQuery) -> Result<LogTailSession, AtmError>;
    fn health(&self) -> Result<ObservabilityHealthSnapshot, AtmError>;
}
```

Implementation rules:

- `atm-core` owns these projected request/result types
- `atm` maps them to and from shared `sc-observability` types
- `atm-core` public APIs must not leak `sc-observability` crate types
- `LogTailSession` stays ATM-owned and synchronous

## 7. Shared-To-ATM Mapping Rules

Initial ATM integration uses the shared logging layer, not typed observation
routing.

Required mapping rules:

- ATM command events emit through `Logger::emit(...)`
- ATM command events use shared `LogEvent` fields for:
  - `service = "atm"`
  - `target` for ATM subsystem grouping
  - `action` for command lifecycle names
  - structured fields for ATM-owned dimensions such as `team`, `actor`,
    `sender`, `message_id`, `task_id`, `outcome`, and `requires_ack`
- ATM failure diagnostics must include a stable ATM-owned error code in shared
  structured fields
- ATM must define those codes in one registry only:
  `crates/atm-core/src/error_codes.rs`, aligned with
  [`../../atm-error-codes.md`](../../atm-error-codes.md)
- ATM-specific query filters map to the shared `LogQuery` surface rather than
  bypassing it with direct JSONL parsing
- `atm log` projections must come from shared `LogEvent` results, not raw
  line-by-line ATM parsing
- `atm doctor` must project shared `LoggingHealthReport` and
  `QueryHealthReport` into ATM findings

## 8. Sink Policy

Required initial sink policy:

- the built-in JSONL file sink must be enabled for retained ATM observability
- the file sink is the authoritative retained record store for `atm log`
- the console sink must remain opt-in for local debug or explicit testing
- normal ATM command output must not be polluted by default console logging

The ATM CLI must continue to own human-readable command rendering separately
from any shared console log sink behavior.

## 9. Command Integration Model

### 9.1 `send`, `read`, `ack`, `clear`

- emit command lifecycle events through the injected boundary
- keep emission best-effort
- never fail the command only because log emission fails
- emit structured failure diagnostics with stable ATM-owned error codes when
  the command fails
- emit structured warning diagnostics with stable ATM-owned error codes when
  the command continues after degraded recovery

### 9.2 `atm log`

- map CLI filters to `AtmLogQuery`
- use `ObservabilityPort::query_logs(...)` for snapshot mode
- use `ObservabilityPort::follow_logs(...)` for tail mode
- render ATM-owned projected records
- return structured `AtmErrorKind::ObservabilityQuery` failures when shared
  query/follow APIs are unavailable or invalid

### 9.3 `atm doctor`

- call `ObservabilityPort::health()`
- treat shared query readiness as a first-class doctor check
- report active log path, logging health, and query health
- distinguish:
  - initialization/config failure
  - query unavailable
  - query degraded
  - healthy

### 9.4 CLI Bootstrap And Parse Failures

- `atm` must log startup/config/bootstrap failures before returning a process
  error
- command-layer argument/validation failures that occur before core-service
  invocation must also log stable ATM-owned error codes

## 10. Testing Obligations

This phase is not complete until observability integration is exercised through
real command tests.

Required coverage:

- unit tests for `atm-core` request/result mapping above a test-double
  `ObservabilityPort`
- unit tests for the central `AtmErrorCode` registry and its mapping from
  `AtmError` / warning sites
- CLI integration tests for `send`, `read`, `ack`, and `clear` that verify
  shared records are emitted into the retained log store
- `atm log` integration tests for:
  - snapshot mode
  - level filtering
  - structured field filtering
  - time-window filtering
  - tail mode
- `atm doctor` integration tests for:
  - healthy logging/query state
  - unavailable file sink/query state
  - degraded query state
- CLI failure-path tests that verify bootstrap, parse, and core-service errors
  are logged with stable ATM-owned error codes
- a live/manual validation pass against a real ATM home before the phase closes

The integration test harness should prefer shared file-backed query/follow
behavior over ATM-local fakes whenever practical.

## 11. Upstream Issue Policy

If ATM integration discovers generic shared-repo friction, the default response
is:

1. file an issue against `sc-observability`
2. reference the issue from ATM planning/design docs when it affects the phase
3. avoid ATM-local compatibility shims unless the shared change would create an
   unreasonable blocker

Examples of legitimate upstream asks:

- logger/test ergonomics that benefit any downstream CLI
- additional sink/writer selection useful for CLI integration
- query/follow helper methods that remain generic and ATM-free

Current tracker opened during this planning pass:

- `sc-observability` issue #55: expose console sink writer selection / stderr
  support for downstream CLI integration and tests

ATM must not push ATM-specific payload, env, or durability semantics down into
the shared repo as a substitute for the ATM adapter boundary.

# Daemon Startup State Machine

## Purpose

Define the executable startup state machine for same-host ATM commands and
`atm-daemon` so every startup transition, failure, and operator-visible outcome
can be communicated from one source of truth.

This document covers:

- CLI-side daemon bootstrap and auto-start attempts
- daemon runtime lifecycle transitions
- startup sub-steps that must complete before serving state
- failure and rollback transitions

This document does not define the delivery event-family state machines. Those
remain in [`../phase-Y/delivery-state-machines.md`](../phase-Y/delivery-state-machines.md).

## Implementation Owners

- CLI bootstrap / auto-start trace:
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm/src/composition.rs`
- daemon runtime lifecycle root:
  - `crates/atm-daemon/src/composition.rs`
- startup dependencies that may fail closed:
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-rusqlite/src/shared_db.rs`

## Machine 1: CLI Bootstrap Trace

This is the client-visible same-host bootstrap machine used by commands like
`doctor`, `send`, `read`, `list`, and `clear` when the daemon is not already
connected.

The CLI reports three summarized outcome lanes:

- `daemon_connect`
- `daemon_launch_gate`
- `daemon_auto_start`

These are surfaced in `BootstrapTraceReport`.

### Connect Lane

State space:

- `NotFound`
- `Connected`
- `Timeout`
- `Failed`

Observed transition outcomes recorded by the client:

- `initial_miss`
  - first local IPC connect attempt failed
  - summarized as `daemon_connect = NotFound`
- `retry_attempt`
  - another bounded connect attempt is being made
  - summarized as `daemon_connect = NotFound` unless a later connect succeeds
- `pending`
  - the daemon is still being awaited after launch
  - summarized as `daemon_connect = NotFound` unless a later connect succeeds
- `connected`
  - local IPC connect succeeded
  - summarized as `daemon_connect = Connected`
- `error`
  - connect path failed with a typed error
  - summarized as `daemon_connect = Failed`

### Launch-Gate Lane

State space:

- `Skipped`
- `Launched`
- `Failed`

Observed transition outcomes recorded by the client:

- `acquired`
  - the client acquired `launch.lock` and is the process allowed to request a
    new daemon launch
  - summarized as `daemon_launch_gate = Launched`
- `contended`
  - another launcher already owns the launch gate
  - summarized as `daemon_launch_gate = Skipped`
- `timeout_exhausted`
  - bounded startup waiting expired
  - summarized as `daemon_launch_gate = Failed`
- `error`
  - launch-gate path failed with a typed error
  - summarized as `daemon_launch_gate = Failed`

### Auto-Start Lane

State space:

- `Skipped`
- `AutoStarted`
- `Failed`

Observed transition outcomes recorded by the client:

- `spawn_requested`
  - the client requested daemon launch
- `publish_wait_started`
  - bounded wait for same-host publish/readiness started
- `publish_wait_continuing`
  - bounded wait is still in progress
- successful later `connected`
  - summarized as `daemon_auto_start = AutoStarted`
- `error`
  - spawn or publish-wait path failed with a typed error
  - summarized as `daemon_auto_start = Failed`
- `timeout_exhausted`
  - bounded publish/connect wait expired
  - summarized as `daemon_auto_start = Failed`

### Bootstrap Rules

- CLI bootstrap does not itself mean the daemon has reached `Running`; it means
  the client observed a successful same-host connect.
- a later `connected` outcome after `spawn_requested` upgrades
  `daemon_auto_start` to `AutoStarted`
- launch-gate ownership and daemon serving-state ownership are different:
  - launch gate serializes client launch attempts
  - owner lock gates daemon serving-state admission

## Machine 2: Daemon Runtime Lifecycle

This is the daemon-owned lifecycle machine in
`crates/atm-daemon/src/composition.rs`.

### States

- `Stopped`
- `Starting`
- `Running`
- `Draining`

### Legal Transitions

- `Stopped -> Starting`
- `Starting -> Running`
- `Starting -> Stopped`
- `Running -> Draining`
- `Draining -> Stopped`

### Illegal Transitions

Examples explicitly rejected by the runtime:

- `Stopped -> Running`
- `Running -> Starting`
- `Stopped -> Draining`
- `Running -> Stopped` without drain path

## Startup Sequence Inside `Stopped -> Starting`

`RuntimeComposition::start()` is the only legal bootstrap entrypoint.

The startup sequence is:

1. emit `start_requested`
2. transition `Stopped -> Starting`
3. run startup replay resume
4. start background lanes
5. prepare the runtime server
6. activate the runtime
7. transition `Starting -> Running`
8. enter serving state

### Step 3: Startup Replay Resume

`resume_startup_replay()` must complete before socket serving begins.

Current rule:

- pending replay work is resumed before the daemon binds/publishes serving
  state
- replay-store assembly is fail-closed

### Step 4: Background Lane Startup

The daemon starts these owned background lanes before serving:

- notification runtime
- reconcile coordinator
- watch runtime
- any other composition-owned runtime lanes required by the current build

If any required lane fails to start:

- startup fails
- rollback attempts lane shutdown
- lifecycle returns to `Stopped`

### Step 5: Runtime Preparation

Runtime preparation covers:

- local IPC transport preparation
- endpoint guard setup
- listener/socket readiness setup

If runtime preparation fails:

- background lanes are shut down as rollback
- lifecycle returns to `Stopped`

### Step 6: Activation

Activation transfers the prepared endpoint guard into runtime ownership.

On success:

- the runtime transitions `Starting -> Running`
- `startup_completed` is emitted

## Shutdown Sequence

The runtime shutdown path is:

1. `Running -> Draining`
2. background lanes receive shutdown
3. runtime serve loop exits
4. lifecycle transitions `Draining -> Stopped`
5. `shutdown_completed` or `shutdown_failed` is emitted

There is no accepted direct `Running -> Stopped` success path.

## Failure and Rollback Transitions

### Startup Failure

Any failure during:

- replay-store assembly
- config validation / peer transport config load
- startup replay resume
- background lane startup
- runtime preparation
- activation before `Running`

must produce:

- `startup_failed`
- lifecycle rollback to `Stopped`

### Serve-Time Failure

Any failure after `Running` must produce:

- `Running -> Draining`
- drain / finalize path
- `Draining -> Stopped`

## Important Current Behavior

`Running` or `doctor` readiness does not imply ATM roster truth is hydrated for
the team.

Current `integrate/phase-Z` behavior:

- daemon startup can succeed
- SQLite can be ready
- canonical ATM roster for a new team may still be empty
- the first daemon-backed `send` can still fail closed on roster-backed harness
  lookup

That is why startup/readiness and first-send readiness are currently different
concepts.

Likewise, `Running` does not imply copied/preexisting SQLite durable state is
compatible. A preexisting `mail.db` can still fail during schema initialization
before the daemon reaches stable serving behavior for the copied-state lane.

## Operator Communication Contract

When communicating startup status, use these layers separately:

1. CLI bootstrap trace
   - did the client find, launch, wait for, or fail to connect to the daemon?
2. daemon lifecycle
   - did the runtime reach `Starting`, `Running`, `Draining`, or `Stopped`?
3. startup dependency result
   - replay store ready?
   - runtime lanes started?
   - SQLite schema/init succeeded?
   - roster truth hydrated or still empty?

Recommended phrasing:

- `daemon auto-started and reached Running, but ATM roster truth is still empty`
- `daemon startup failed during SQLite schema initialization on copied durable state`
- `daemon did not start; launch gate was contended`

Avoid collapsing these into one vague statement like `startup failed` unless
the exact failing transition is still unknown.

## Summary Table

| Layer | State / Outcome | Meaning |
| --- | --- | --- |
| CLI bootstrap | `daemon_connect = NotFound` | no same-host daemon connection yet |
| CLI bootstrap | `daemon_launch_gate = Launched` | this client owned launch admission |
| CLI bootstrap | `daemon_auto_start = AutoStarted` | client launched daemon and later connected |
| daemon lifecycle | `Starting` | replay/lane/runtime preparation in progress |
| daemon lifecycle | `Running` | daemon accepted activation and entered serve path |
| daemon lifecycle | `Draining` | shutdown path is in progress |
| daemon lifecycle | `Stopped` | runtime is fully stopped or rolled back |
| startup dependency | SQLite ready | schema/init succeeded for the active lane |
| startup dependency | roster hydrated | ATM roster truth exists for the target team |


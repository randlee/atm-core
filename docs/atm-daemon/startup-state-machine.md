# Daemon Startup State Machines

## Purpose

Define the executable startup and first-use state machines for same-host ATM
commands and `atm-daemon` so every transition, failure, and operator-visible
outcome can be communicated from one source of truth.

This document covers three separate machines:

- CLI-side daemon bootstrap and auto-start attempts
- daemon runtime lifecycle transitions
- reachable-daemon command and `doctor` outcomes

Diagram source:

- [cli-bootstrap-state-machine.mmd](./cli-bootstrap-state-machine.mmd)
- [runtime-lifecycle-state-machine.mmd](./runtime-lifecycle-state-machine.mmd)
- [reachable-daemon-request-outcome-state-machine.mmd](./reachable-daemon-request-outcome-state-machine.mmd)

These machines do not define the delivery event-family state machines. Those
remain in [`../phase-Y/delivery-state-machines.md`](../phase-Y/delivery-state-machines.md).

## Testability Rule

Each machine must satisfy:

- every node is a code-owned state or terminal output
- every edge is a real event or guard QA can force in a test
- every terminal maps to a caller-visible result or `DoctorReport` result
- no branch label may use prose like `may still` or `fails closed`

## Implementation Owners

- CLI bootstrap / auto-start trace:
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm/src/composition.rs`
- daemon runtime lifecycle root:
  - active: `atm_daemon_bootstrap::run_replacement_daemon_with_observability`
  - historical/retired: `crates/atm-daemon/src/composition.rs` (deleted by
    AM.3)
- startup dependencies that may fail:
  - `crates/atm-rusqlite/src/shared_db.rs`
## Machine 1: CLI Bootstrap

Source:

- [cli-bootstrap-state-machine.mmd](./cli-bootstrap-state-machine.mmd)

Code:

- `crates/atm/src/composition.rs::CliComposition::bootstrap`
- `crates/atm-daemon-client/src/lib.rs::DaemonSupervisor`

This machine ends before any request is sent. If it fails, the caller gets a
typed bootstrap error and `doctor` does not return a report.

## Machine 2: Daemon Runtime Lifecycle

Source:

- [runtime-lifecycle-state-machine.mmd](./runtime-lifecycle-state-machine.mmd)

Historical retired code (deleted by AM.3):

- `crates/atm-daemon/src/composition.rs::RuntimeLifecycle`
- `crates/atm-daemon/src/composition.rs::RuntimeComposition::start`

The active replacement bootstrap owns the legal daemon lifecycle transitions
and fail-closed rollback to `Stopped`.

## Machine 3: Reachable-Daemon Request Outcomes

Source:

- [reachable-daemon-request-outcome-state-machine.mmd](./reachable-daemon-request-outcome-state-machine.mmd)

Code:

- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-rusqlite/src/shared_db.rs`

This machine starts only after bootstrap has already reached a live daemon. It
defines what the caller sees versus what `doctor` can explain when runtime,
roster, or copied-state issues are encountered.
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

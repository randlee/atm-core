# ATM Daemon Logging Contract

This document is the Phase S.9 logging contract for ATM-owned retained logs.
It is a planning/specification document until the S.9 implementation lands.

## Default Retained Log Path

ATM-owned retained logs default to:

- `~/.atm/logs/atm.log.jsonl`

The retained log directory is host-scoped and independent of `ATM_HOME`.

## Override

`ATM_LOG_DIR` overrides the exact retained log directory.

Examples:

- default:
  - `~/.atm/logs/atm.log.jsonl`
- override:
  - `ATM_LOG_DIR=~/atm-test-logs`
  - retained log path: `~/atm-test-logs/atm.log.jsonl`

## `ATM_LOG_DIR` Validation

`ATM_LOG_DIR` is accepted only when all of the following hold:

- it resolves to an absolute path
- empty string is treated as unset
- it does not overlap with `~/.atm/daemon/`
- it does not resolve under `~/.claude/`
- it targets a local filesystem

## Default Retained Event Set

Without extra operator tuning, retained logging must include at least:

- daemon start requested
- daemon startup completed / ready
- daemon shutdown requested
- daemon shutdown completed
- daemon degraded / abnormal-exit signals
- every `warn!` event emitted by ATM subsystems
- every `error!` event emitted by ATM subsystems

Degraded-state and abnormal-exit signals live in the default `warn!` /
`error!` baseline rather than in a second info-only lifecycle family.

This contract is intentionally minimal. It is not a requirement to retain
every possible `info!` event across the whole system, but the daemon lifecycle
milestones above must remain visible by default.

## Sink Initialization Failure Behavior

Startup is fail-closed:

- if the retained log directory cannot be created or opened at daemon start,
  the daemon does not continue in degraded logging mode
- the startup error must be reported to stderr with the full path and OS error
- implementation must not rely on `unwrap` / `expect` for retained-log sink
  creation

Recovery guidance:

- operator fixes permissions, disk state, or `ATM_LOG_DIR`
- daemon restart is then retried explicitly

## Mid-Run Write Failure Behavior

Normal-operation retained-log write failures are fail-open:

- if disk-full or similar write failure occurs after startup, the daemon
  reports the failure to stderr and continues operation
- one retained-log write failure does not terminate the daemon
- repeated identical write failures may be rate-limited, but the first failure
  must be surfaced

## Shutdown Flush Contract

Graceful shutdown attempts one best-effort retained-log flush with a bounded
2-second timeout.

Rules:

- timeout is preferred to indefinite daemon shutdown blocking
- partial flush on timeout is acceptable
- unflushed retained-log events may be lost during timeout exit

Cross-reference:

- `docs/architecture.md` graceful-shutdown best-effort flush policy

## Rotation / Size Cap

V1 defines no built-in retained-log rotation.

Current operator contract:

- ATM will continue appending to the retained log file
- operators are responsible for external rotation or quota management
- a filesystem with quota or external log management is recommended

Future work:

- a later sprint may introduce bounded rotation if operator evidence justifies
  it

## Watcher / Reconcile Exclusion

Retained-log writes under `~/.atm/logs/` must not trigger mailbox watcher or
reconcile events.

The implementation sprint must therefore prove:

- log-directory paths are excluded from watcher trigger roots
- appending to `~/.atm/logs/atm.log.jsonl` does not create a reconcile event
  during ordinary daemon operation

## Console Sink

The shared console sink remains off by default for ordinary ATM CLI command
execution so retained logs do not pollute normal stdout/stderr command output.

## Forbidden Default Paths

ATM-owned retained logs must not default to:

- `~/logs/`
- `~/.claude/logs/`
- `.local/share/logs/`

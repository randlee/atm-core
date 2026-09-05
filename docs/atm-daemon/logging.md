# ATM Daemon Retained-Log Contract

This is the active retained-log contract for the Tokio/Axum daemon and its
CLI and graft consumers.

## Guarantees

Every ATM `warn!` and `error!` event is eligible for retention. INFO is
additionally retained only for `atm_daemon_bootstrap::lifecycle`,
`atm_http_runtime::listener`, and `atm_storage_rusqlite::maintenance`. The
bridge permits only `RETAINED_FIELD_ALLOWLIST`, so retained records never
contain message bodies, recipients, tokens, raw environment/configuration, or
absolute user paths.

## Exceptions and origin rules

`PRE_BOOTSTRAP_STDERR_ALLOWLIST` is empty. A future pre-bootstrap stderr
exception must be named in that allowlist; after bootstrap, runtime code uses
the tracing bridge. `origin` records diagnostic provenance (`tracing`,
`sqlite`, `timeline`, or a documented adapter value), not user input.

## Loss and degradation

The JSONL logger and SQLite timeline are independently bounded, non-blocking
sinks. `dropped_queue_full_total`, `dropped_reentrant_total`, and
`dropped_persist_error_total` are durable degradation evidence. Timeline
transitions use `ATM_LOG_SINK_DEGRADED` and `ATM_LOG_SINK_RECOVERED`; its
batch maximum is `DIAGNOSTIC_BATCH_MAX = 128`, its flush interval is
`DIAGNOSTIC_FLUSH_INTERVAL_MS = 250`, and the degradation recovery and
transition rate-limit windows are `DEGRADATION_RECOVERY_WINDOW_SECS = 60` and
`DEGRADATION_RATE_LIMIT_SECS = 60`. A merged view is therefore not lossless
under overload. See [graft observability](../graft-observability.md) for the
fallback satellite contract.

## Default Retained Log Path

ATM-owned retained logs default to:

- `{ATM_HOME}/.atm/logs/atm.log.jsonl`

Without `ATM_LOG_DIR`, the retained log directory is derived from the accepted
`ATM_HOME` root for the active installation.

## Override

`ATM_LOG_DIR` overrides the exact retained log directory.

Examples:

- default:
  - `{ATM_HOME}/.atm/logs/atm.log.jsonl`
- override:
  - `ATM_LOG_DIR=~/atm-test-logs`
  - retained log path: `~/atm-test-logs/atm.log.jsonl`

## `ATM_LOG_DIR` Validation

`ATM_LOG_DIR` is accepted only when all of the following hold:

- it resolves to an absolute path
- empty string is treated as unset
- it does not overlap with `{ATM_HOME}/.atm/daemon/`
- it does not resolve under `~/.claude/`
- it targets a local filesystem

## Default Retained Event Set

Without extra operator tuning, the replacement Tokio/Axum daemon retains:

- daemon start requested
- daemon startup completed / ready
- daemon shutdown requested
- daemon shutdown completed
- daemon degraded / abnormal-exit signals
- every `warn!` and `error!` event emitted by ATM subsystems
- `info!` from `atm_daemon_bootstrap::lifecycle`,
  `atm_http_runtime::listener`, and `atm_storage_rusqlite::maintenance`

The tracing bridge retains only the allowlisted structured fields: `ts`,
`level`, `component`, `code`, `action`, `correlation_id`, `outcome`,
`elapsed_ms`, `attempt`, `strategy`, `endpoint_kind`, `failure_class`,
`error_layer`, `origin`, `message`, and `detail`. It drops every other field,
including message bodies, recipients, tokens, raw environment/configuration,
and absolute user paths. Admission is non-blocking: a full logger queue drops
the record and increments the retained diagnostic drop counter. Events emitted
while the bridge is writing are dropped to prevent recursion.

No post-bootstrap runtime path writes directly to stderr. The documented
`PRE_BOOTSTRAP_STDERR_ALLOWLIST` is currently empty; a future exception must
be a named pre-logger `file:function` entry here and in the lint gate.

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

The daemon retained log uses explicit built-in rotation:

- active file rotation at `10 MiB`
- retain the newest `5` rotated files beside the active log
- prune rotated files older than `7` days

Operator contract:

- ATM keeps a bounded recent daemon history under the host-scoped log root
- operators may still layer external rotation or quota management if they need
  stricter retention

## Non-Blocking Write Constraint

ADR-011 forbids retained-log writes from blocking async executor threads.

`atm-daemon` satisfies that requirement by architecture rather than by an
internal sink worker thread:

- the daemon does not route retained-log writes through an async executor
- `sc-observability =1.2.0` `JsonlFileSink` performs synchronous local-file
  writes on the daemon's ordinary OS threads
- shutdown flush runs on a dedicated bounded finalizer thread

That keeps retained logging out of async-executor hot paths while preserving
the fail-closed bootstrap and fail-open mid-run behavior required by ADR-011.

## Watcher / Reconcile Exclusion

Retained-log writes under `{ATM_HOME}/.atm/logs/` must not trigger mailbox watcher or
reconcile events.

The implementation sprint must therefore prove:

- log-directory paths are excluded from watcher trigger roots
- appending to `{ATM_HOME}/.atm/logs/atm.log.jsonl` does not create a
  reconcile event
  during ordinary daemon operation

## Console Sink

The shared console sink remains off by default for ordinary ATM CLI command
execution so retained logs do not pollute normal stdout/stderr command output.

## Forbidden Default Paths

ATM-owned retained logs must not default to:

- `~/logs/`
- `~/.claude/logs/`
- `.local/share/logs/`

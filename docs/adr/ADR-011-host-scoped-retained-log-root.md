# ADR-011 — Host-Scoped Retained Log Root

| Field | Value |
| --- | --- |
| ID | ADR-011 |
| Status | Accepted |
| Date | 2026-05-10 |
| Deciders | arch-ctm, team-lead |
| Relates-to | ADR-010 |
| Supersedes | — |

## Context

Phase S.8 left ATM retained logging in an operationally weak state:

- runtime/state files are host-scoped under `~/.atm/...`
- retained log files still default to non-ATM-owned locations
- daemon lifetime visibility depends too much on implicit logger defaults

That split makes routine support work harder than it needs to be. Operators
should have one ATM-owned host-scoped place to look for retained diagnostics,
and the default retained event set must be non-silent for daemon startup,
shutdown, degradation, and subsystem warnings/errors.

## Decision

ATM retained logs are host-scoped and default to:

- `~/.atm/logs/atm.log.jsonl`

Auxiliary ATM-owned retained logs also live under the same host-scoped log
directory unless a later ADR makes a narrower exception.

The retained log directory is resolved independently of `ATM_HOME`:

- default root: OS user home + `~/.atm/logs/`
- override: `ATM_LOG_DIR`

`ATM_LOG_DIR` is the exact retained log directory, not a parent root that
requires another implicit `logs/` segment.

`ATM_LOG_DIR` validation rules:

- it must resolve to an absolute path
- empty string is treated as unset
- it must not overlap with `~/.atm/daemon/`
- it must not resolve under `~/.claude/`

The retained log directory contract supports only local filesystems.
Network-mounted paths are out of scope for V1; behavior on NFS/CIFS is
undefined.

Default retained logging must preserve at least this baseline:

- daemon start requested
- daemon startup completed / serving ready
- daemon shutdown requested
- daemon shutdown completed
- daemon degraded / abnormal-exit signals
- every `warn!` and `error!` event emitted by ATM subsystems

The default retained logger level must therefore be high enough to include the
daemon lifecycle `info!` events in addition to warning/error events. The
console sink remains off by default for ordinary CLI command execution.

## Failure Policy

Startup behavior:

- if the retained log directory cannot be created or opened at daemon startup,
  the daemon fails closed
- the failure is reported to stderr with the full path and the underlying OS
  error

Mid-run behavior:

- if a retained log write fails after startup, ATM degrades to best-effort
  stderr reporting and continues normal daemon/CLI operation
- one retained-log write failure does not terminate the daemon

Shutdown behavior:

- graceful shutdown attempts one best-effort retained-log flush with a bounded
  2-second timeout
- timeout or partial-flush loss is accepted over indefinite shutdown blocking

## Rotation Policy

V1 does not define built-in retained-log rotation.

Operator guidance:

- retained logs may grow without automatic rollover
- use a filesystem with quota or external log management if growth matters

Future work:

- a later sprint may add bounded size/rotation if operator evidence justifies
  it

## Implementation Constraints

- retained-log writes must not block async executor threads
- the implementation must use a dedicated sync writer thread or an explicit
  blocking task boundary
- watcher/reconcile paths must exclude `~/.atm/logs/` so retained-log appends
  cannot create false mailbox/reconcile churn

## Consequences

- `atm-core` must own a host-scoped log-directory helper distinct from
  `atm_home()`
- `atm` observability bootstrap must route retained logs through the
  host-scoped ATM log directory rather than non-ATM-owned defaults
- `atm-daemon` health/reporting must surface the same retained log path
- daemon and CLI logging docs must describe one consistent default path and
  default retained event baseline

## Alternatives Considered

### Keep Non-ATM-Owned Default Log Roots

Rejected because it separates retained logs from the rest of ATM host-scoped
runtime state and weakens operator discoverability.

### Make `ATM_HOME` Own Retained Logs

Rejected because retained diagnostics are host-scoped operational state, not
team/workspace-local Claude layout state.

### Keep Default Logging Quiet And Require `ATM_LOG`

Rejected because daemon lifetime and degradation events must be available by
default when operators are debugging startup/shutdown/runtime faults.

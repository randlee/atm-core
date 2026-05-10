# ADR-011 — Host-Scoped Retained Log Root

## Status

Accepted

## Context

Phase S.8 left ATM retained logging in an operationally weak state:

- runtime/state files are host-scoped under `~/.atm/...`
- retained log files still default to `.local/share/logs/...`
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

## Consequences

- `atm-core` must own a host-scoped log-directory helper distinct from
  `atm_home()`
- `atm` observability bootstrap must route retained logs through the
  host-scoped ATM log directory rather than `.local/share/logs`
- `atm-daemon` health/reporting must surface the same retained log path
- daemon and CLI logging docs must describe one consistent default path and
  default retained event baseline

## Alternatives Considered

### Keep `.local/share/logs`

Rejected because it separates retained logs from the rest of ATM host-scoped
runtime state and weakens operator discoverability.

### Make `ATM_HOME` Own Retained Logs

Rejected because retained diagnostics are host-scoped operational state, not
team/workspace-local Claude layout state.

### Keep Default Logging Quiet And Require `ATM_LOG`

Rejected because daemon lifetime and degradation events must be available by
default when operators are debugging startup/shutdown/runtime faults.

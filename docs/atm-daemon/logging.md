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
  - `ATM_LOG_DIR=/tmp/atm-logs`
  - retained log path: `/tmp/atm-logs/atm.log.jsonl`

## Default Retained Event Set

Without extra operator tuning, retained logging must include at least:

- daemon start requested
- daemon startup completed / ready
- daemon shutdown requested
- daemon shutdown completed
- daemon degraded / abnormal-exit signals
- every `warn!` event emitted by ATM subsystems
- every `error!` event emitted by ATM subsystems

This contract is intentionally minimal. It is not a requirement to retain every
possible `info!` event across the whole system, but the daemon lifecycle
milestones above must remain visible by default.

## Console Sink

The shared console sink remains off by default for ordinary ATM CLI command
execution so retained logs do not pollute normal stdout/stderr command output.

## Forbidden Default Paths

ATM-owned retained logs must not default to:

- `~/logs/`
- `~/.claude/logs/`
- `.local/share/logs/`

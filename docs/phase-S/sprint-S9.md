# Phase S.9 — Host-Scoped Logging Defaults

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.9"
status: planned
estimated_scope: S
```

## Goal

Finish Phase S retained logging by moving ATM-owned retained logs to the
host-scoped ATM state root and defining the minimum default retained event set
that must be present without extra operator tuning.

## Governing Requirements

- `REQ-P-OBS-001`
- `REQ-P-OBS-002`
- `REQ-P-OBS-003`
- `REQ-ATM-OBS-001`
- `REQ-DAEMON-OBS-001`
- `REQ-DAEMON-OBS-002`

## Governing ADRs

- `docs/adr/ADR-011-host-scoped-retained-log-root.md`

## Hard Dependencies

- S.8 is merged
- the shared `sc-observability` integration remains the retained log/query
  substrate
- host-scoped runtime/state ownership remains rooted under `~/.atm/`

## Required Work

1. Add one host-scoped ATM log-directory contract.
1.1 Add `host_log_dir_from_home(home_dir: &Path) -> PathBuf` in
    `atm-core/src/home.rs`.
1.2 Add `host_log_dir() -> Result<PathBuf, AtmError>` that honors
    `ATM_LOG_DIR` first, then falls back to the OS user home.
1.3 Keep retained logs host-scoped and independent of `ATM_HOME`.

2. Move retained ATM logs to the ATM host state root.
2.1 Default retained log file:
    - `~/.atm/logs/atm.log.jsonl`
2.2 Auxiliary ATM-owned retained log files remain under `~/.atm/logs/`
    unless a later ADR narrows them.
2.3 No retained ATM log writes may default to:
    - `~/logs/`
    - `~/.claude/logs/`
    - `.local/share/logs/`

3. Define the default retained event baseline.
3.1 Retain daemon lifecycle `info!` events by default:
    - start requested
    - startup completed / ready
    - shutdown requested
    - shutdown completed
3.2 Retain every `warn!` and `error!` event across ATM subsystems by default.
3.3 Keep the shared console sink disabled by default for ordinary CLI command
    execution.

4. Document the retained logging contract.
4.1 Add `docs/atm-daemon/logging.md`.
4.2 Document the default retained path and `ATM_LOG_DIR`.
4.3 Document the minimum retained event set.
4.4 Cross-link the daemon logging contract from top-level and crate-local docs.

## Required Code Targets

- `crates/atm-core/src/home.rs`
- `crates/atm/src/main.rs`
- `crates/atm/src/observability.rs`
- `crates/atm-daemon/src/runtime_health.rs`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plan-phase-S.md`
- `docs/phase-S/issues.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/logging.md`
- `docs/adr/ADR-011-host-scoped-retained-log-root.md`
- `docs/adr/INDEX.md`

## Acceptance Criteria

- ATM retained logs default to `~/.atm/logs/atm.log.jsonl`
- `ATM_LOG_DIR` redirects the exact retained log directory
- retained logging is host-scoped and independent of `ATM_HOME`
- default retained logging includes daemon lifecycle `info!` events plus all
  `warn!` / `error!` events across ATM subsystems
- the shared console sink remains off by default for ordinary CLI command
  execution
- no ATM-owned retained log defaults point at `~/logs/`, `~/.claude/logs/`,
  or `.local/share/logs/`

## Required Validation

- `just lint`
- `cargo test -p agent-team-mail-core`
- `cargo test -p atm-daemon`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`

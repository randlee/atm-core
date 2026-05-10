# Phase S.9 — ATM Default Logging Behavior

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.9"
status: planned
estimated_scope: S
```

## Goal

Document and implement correct ATM default logging behavior: move the default
retained log path from the bare `~/logs/` root to `~/.atm/logs/`, define the
minimal retained event set, and add `host_log_dir_from_home` to
`atm-core/src/home.rs` to give logging the same canonical path helper as
daemon and db directories.

## Background

`DaemonObservability` in `crates/atm-daemon/src/runtime_health.rs` constructs
its log path as `home_dir.join("logs").join("atm.log.jsonl")`, placing logs
in `~/logs/atm.log.jsonl` (off the OS home root). All other ATM-owned
directories live under `~/.atm/` (`~/.atm/daemon/`, `~/.atm/db/`). Logs
should follow the same convention: `~/.atm/logs/`.

The `$ATM_LOG_DIR` environment variable (or equivalent config key) should be
able to redirect the log path, matching the redirect pattern used for
`$ATM_HOME`.

## Governing Requirements

- `REQ-P-RELIABILITY-001` (daemon operational observability)
- `REQ-CORE-CONFIG-001` (config ownership in atm-core)

## Hard Dependencies

- S.8 merged (config infrastructure available)

## Required Work

1. Add `host_log_dir_from_home` to `crates/atm-core/src/home.rs`.
   1.1 `host_log_dir_from_home(home_dir: &Path) -> PathBuf` returns
       `home_dir.join(".atm").join("logs")`.
   1.2 Add corresponding `host_log_dir() -> Result<PathBuf, AtmError>` that
       uses `resolve_user_home()` (not `atm_home()`), consistent with
       `host_runtime_dir` and `host_db_dir` — logs are host-scoped, not
       team-scoped.
   1.3 Add unit tests matching the pattern in `home.rs` tests.

2. Update `DaemonObservability` in `crates/atm-daemon/src/runtime_health.rs`.
   2.1 Replace `self.home_dir.join("logs").join("atm.log.jsonl")` at lines 82
       and 136 with `host_log_dir_from_home(&self.home_dir).join("atm.log.jsonl")`.
   2.2 Ensure the log directory is created if absent before opening the file.

3. Support log path redirection.
   3.1 Honor `ATM_LOG_DIR` environment variable as an override for the log
       directory when set and non-empty.
   3.2 Add `host_log_dir()` to read `ATM_LOG_DIR` first, then fall back to
       `host_log_dir_from_home(&resolve_user_home()?)`.

4. Define and document the minimal retained event set.
   4.1 Retained log must capture at minimum:
       - daemon start / stop lifecycle events
       - all `warn!` and `error!` level events across all subsystems
   4.2 Document the retained event set in `docs/atm-daemon/logging.md`
       (create if absent).
   4.3 Verify the daemon's tracing subscriber configuration emits at least
       these events to the retained log.

## Required Code Targets

- `crates/atm-core/src/home.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `docs/atm-daemon/logging.md` (create)

## Acceptance Criteria

- `just lint` PASS
- `cargo test -p atm-core` PASS (home.rs tests cover `host_log_dir_from_home`)
- `cargo test -p atm-daemon` PASS
- Default log path is `~/.atm/logs/atm.log.jsonl`
- `ATM_LOG_DIR` overrides the log directory
- `docs/atm-daemon/logging.md` documents the retained event set
- No log files written to `~/logs/` or `~/.claude/logs/`

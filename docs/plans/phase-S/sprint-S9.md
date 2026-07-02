# Phase S.9 — Host-Scoped Logging Defaults

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.9"
status: in-review
estimated_scope: S
```

## Goal

Finish Phase S retained logging by moving ATM-owned retained logs to the
host-scoped ATM state root, defining the minimum default retained event set,
and preventing log-directory activity from feeding false watcher/reconcile
signals back into mailbox handling.

## Governing Requirements

- `REQ-P-OBS-001`
- `REQ-P-OBS-002`
- `REQ-P-OBS-003`
- `REQ-ATM-OBS-001`
- `REQ-DAEMON-OBS-001`
- `REQ-DAEMON-OBS-002`

## Governing ADRs

- `docs/adr/ADR-011-host-scoped-retained-log-root.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Hard Dependencies

- S.8 is merged
- the shared `sc-observability` integration remains the retained log/query
  substrate
- host-scoped runtime/state ownership remains rooted under `~/.atm/`

## Planning-Phase Deliverables

This planning commit is docs-only:

- sprint-S9 implementation plan
- ADR-011
- product/crate logging contract alignment

This commit must not implement `ARCH-002` through `ARCH-005`. Those are
pre-existing code contradictions already listed in Required Code Targets and
belong to the implementation sprint on this same worktree after plan review.

## Required Work

1. Add one host-scoped ATM log-directory contract.
1.1 Add `host_log_dir_from_home(home_dir: &Path) -> PathBuf` in
    `atm-core/src/home.rs`.
1.2 Add `host_log_dir() -> Result<PathBuf, AtmError>` that honors
    `ATM_LOG_DIR` first, then falls back to the OS user home.
1.3 Keep retained logs host-scoped and independent of `ATM_HOME`.
1.4 `host_log_dir_from_home(...)` returns a raw `PathBuf`; no `AtmLogDir`
    newtype is required for V1, matching the existing host-runtime and host-db
    helper style.
1.5 Failure inventory for `host_log_dir()`:
    - home dir unresolvable -> typed config error
    - `ATM_LOG_DIR` set but empty or non-UTF-8 -> typed config error
    - log-dir creation/open failure -> typed I/O/config startup failure with
      path context
    - recovery guidance for all three: fail closed, log to stderr, operator
      repair required before retry
1.6 Forbidden-path invariant:
    - the resolved retained log path must never live under `~/.claude/`

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
3.3 Degraded-state and abnormal-exit signals are covered by the default
    `warn!` / `error!` baseline rather than by a second info-only lifecycle
    family.
3.4 Keep the shared console sink disabled by default for ordinary CLI command
    execution.

4. Document the retained logging contract.
4.1 Add `docs/atm-daemon/logging.md`.
4.2 Document the default retained path and `ATM_LOG_DIR`.
4.3 Document the minimum retained event set.
4.4 Document startup fail-closed sink initialization, mid-run fail-open write
    behavior, local-filesystem-only support, and bounded shutdown flush rules.
4.5 Document V1 log rotation as a known limitation rather than an implemented
    feature.
4.6 Cross-link the daemon logging contract from top-level and crate-local docs.

5. Ensure log-directory writes do not trigger spurious mailbox/watcher events.
5.1 Verify the daemon filesystem watcher excludes `~/.atm/logs/` from the set
    of paths that trigger reconcile or inbox-import events.
5.2 Verify retained-log appends do not create new-mail churn when the daemon
    is otherwise idle.
5.3 Keep the watcher exclusion rule documented in Phase S closeout and project
    summaries so the implementation sprint cannot silently drop it.

## Required Code Targets

- `crates/atm-core/src/home.rs`
- `crates/atm/src/main.rs`
- `crates/atm/src/observability.rs`
- `crates/atm-daemon/src/runtime_health.rs`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plans/phase-S/plan-phase-S.md`
- `docs/plans/phase-S/issues.md`
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

Planning-branch closeout:

- `just lint` PASS
- all changed planning docs are internally consistent
- `ARCH-002` through `ARCH-005` remain untouched on this commit

Implementation-sprint acceptance:

- ATM retained logs default to `~/.atm/logs/atm.log.jsonl`
- `ATM_LOG_DIR` redirects the exact retained log directory
- retained logging is host-scoped and independent of `ATM_HOME`
- default retained logging includes daemon lifecycle `info!` events plus all
  `warn!` / `error!` events across ATM subsystems
- console sink remains disabled by default; no retained-log output appears on
  stdout/stderr during normal `atm send` / `atm read` runs unless an explicit
  logging opt-in such as `ATM_LOG` is set
- sink initialization fails closed at startup with typed path/context errors
- mid-run retained-log write failures degrade to stderr-once plus continued
  operation rather than daemon termination
- no ATM-owned retained log defaults point at `~/logs/`, `~/.claude/logs/`,
  or `.local/share/logs/`
- no reconcile event fires when a normal retained-log append writes to
  `~/.atm/logs/atm.log.jsonl`
- `atm doctor` surfaces the resolved retained log path

## Required Validation

- `just lint`
- `cargo test -p agent-team-mail-core`
- `cargo test -p atm-daemon`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`

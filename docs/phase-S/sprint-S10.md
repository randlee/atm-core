# Sprint S.10 — Daemon Retained Logger Bootstrap

**Branch**: feature/pS-s10-daemon-retained-logger  
**Base**: integrate/phase-S @ 3151d4b  
**PR target**: integrate/phase-S  
**Status**: Implementation

## Goal

Wire the shared sc-observability retained logger into `atm-daemon`. S.9 landed path resolution and validation (`host_log_dir`, ADR-011) but the daemon entrypoint never bootstraps the live adapter. Daemon lifecycle events do not reach `~/.atm/logs/atm.log.jsonl`, and `atm doctor` reports Healthy observability from a no-op stub.

## Required Work

### 1. Bootstrap retained logger in daemon entrypoint

`crates/atm-daemon/src/main.rs` — replace the stderr-only `tracing_subscriber` setup with the shared sc-observability adapter used by the `atm` CLI:

- Call `new_adapter_port(home_dir, stderr_logs)` (or equivalent daemon variant) at daemon startup
- Route daemon lifecycle, request, and runtime events through the live retained-file sink
- Default log file: `~/.atm/logs/atm.log.jsonl` (via `host_log_dir()`)
- Respect `ATM_LOG` env var for level filtering (same as CLI)
- Fail-closed: if the retained logger cannot be initialized, abort daemon startup with a clear error

### 2. Wire doctor health to live sink health

`crates/atm-daemon/src/runtime_health.rs` — replace the synthetic `DaemonObservabilityPort` stub:

- `emit()` must route to the live retained logger
- `health()` must reflect real sink health (file writable, no I/O errors) rather than the `ATM_OBSERVABILITY_RETAINED_SINK_FAULT` test knob
- The test knob (`ATM_OBSERVABILITY_RETAINED_SINK_FAULT`) may remain for test-only fault injection but must not be the primary health source in production builds

### 3. Integration test

Add a test proving that `atm-daemon` startup/shutdown lifecycle events land in the retained log file:
- Spin up daemon with a temp `home_dir`
- Verify `~/.atm/logs/atm.log.jsonl` exists and contains at least one event after startup
- Verify `atm doctor` reports healthy observability when the file is writable

## Acceptance Criteria

- `atm-daemon` writes lifecycle events to `~/.atm/logs/atm.log.jsonl` by default
- `atm doctor` health reflects real retained-sink state, not the fault-injection stub
- Daemon startup fails-closed when `host_log_dir()` returns Err
- daemon-side retained log query/follow remain explicitly deferred to the
  CLI-owned log surface for S.10
- `just lint` PASS
- `cargo test -p atm-daemon` PASS
- `cargo test -p atm` PASS
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc` PASS

## References

- `docs/adr/ADR-011-host-scoped-retained-log-root.md` — host-scoped log root contract
- `docs/atm-daemon/requirements.md:171-178` — daemon observability requirements
- Arch-ctm prod review TASK-1214-PROD-REVIEW finding #1 (P1)
- `crates/atm-daemon/src/runtime_health.rs` — daemon doctor/runtime-health projection
- `crates/atm/src/main.rs` — reference implementation of sc-observability bootstrap

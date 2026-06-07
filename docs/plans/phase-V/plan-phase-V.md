# Phase V — Daemon Hardening And Boundary Cleanup

Goal:
- close the daemon hardening follow-on work identified in the Phase U
  post-mortem before daemon observability and runtime failure handling are
  treated as settled enough for system testing
- close `ARCH-PU-002` by redesigning daemon observability as a bottom-of-stack
  sink boundary rather than a daemon-wide event reconstruction layer
- migrate and streamline observability so the system has clear runtime signals
  during testing
- tighten daemon-unavailable recovery contracts so failures are actionable

Planning branch:
- `feature/daemon-hardening-plan`

Expected execution shape:
- `V.1` defines the final observability boundary and event model
- `V.2` migrates event ownership into the daemon subsystems
- `V.3` removes old central mapping code and streamlines the final shape
- `V.4` hardens daemon/client/runtime recovery guidance for testing-critical
  failures

Execution sequence:
- `V.1` must land first because it defines the event contract and bottom-of-stack
  observability boundary
- `V.2` depends on `V.1` and owns the subsystem-by-subsystem migration
- `V.3` depends on `V.2` and owns explicit deletion/consolidation of the old
  central mapping path
- `V.4` may begin once the observability line is stable enough that daemon and
  client failure paths can be reviewed against the final testing surface

Rationale:
- `RULE-002` / `ARCH-PU-002` around
  `crates/atm-daemon/src/daemon_observability.rs` `emit_runtime_event`
  drive the observability boundary/model, migration, and cleanup sequence in
  `V.1` through `V.3`
- `QA-U-002` plus `RBP-PU-001` / `RBP-PU-002` drive `V.4` recovery context
  hardening because system testing needs actionable daemon/runtime failures

Authoritative sprint sequence:
- `docs/plans/phase-V/sprint-V1.md`
- `docs/plans/phase-V/sprint-V2.md`
- `docs/plans/phase-V/sprint-V3.md`
- `docs/plans/phase-V/sprint-V4.md`

Sprint summary:
- `V.1` observability boundary and event model
- `V.2` observability migration into subsystems
- `V.3` observability removal and streamlining
- `V.4` recovery context hardening

In-scope ownership split:
- `V.1` owns the final daemon observability trait, event model, and bottom-of-stack
  boundary rule
- `V.2` owns the subsystem migration touchpoints listed below
- `V.3` owns deletion and consolidation of the old central mapping path
- `V.4` owns daemon/client/runtime recovery guidance hardening
- `ARCH-PU-002` / `RULE-002` remain in-scope across `V.1` through `V.3`; they
  are not deferred to backlog

Observability migration touchpoints:
- shared observability boundary and composition:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs`
  - `crates/atm-daemon/src/daemon_observability.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/main.rs`
  - `crates/atm-daemon/src/lib.rs`
- daemon subsystems that must own or review their own event emission:
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `crates/atm-daemon/src/advisory_runtime.rs`
  - `crates/atm-daemon/src/notification_runtime.rs`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-daemon/src/watch_runtime.rs`
  - `crates/atm-daemon/src/reconcile_runtime.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/host_ownership.rs`
  - `crates/atm-daemon/src/lifecycle_control.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
- daemon observability test/support surfaces that must follow the final shape:
  - `crates/atm-daemon/src/test_observability.rs`
  - `crates/atm-daemon/src/runtime_health_test_support.rs`
  - `crates/atm-daemon/src/test_support.rs`
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/tests_advisory.rs`
  - `crates/atm-daemon/src/tests_lifecycle.rs`

Code flagged for removal or consolidation:
- `crates/atm-daemon/src/daemon_observability.rs`
  - `emit_runtime_event(...)`
  - `map_command_event(...)`
  - `map_runtime_event(...)`
  - centralized runtime-event level mapping and wording policy
- any daemon-wide helper path that reconstructs subsystem meaning after the
  fact instead of receiving a subsystem-owned event payload
- any duplicate test-support event shaping that only exists to preserve the
  old central mapping model

Deferred backlog items:
- runtime test isolation lint
  - source finding: `FTQ-U9-001`
  - GH issue: `#259`
- workspace `#[path]` lint
  - GH issue: `#260`
- sprint-close hygiene gate
  - source findings: `ATM-QA-PU-001` through `ATM-QA-PU-005`
  - GH issue: `#261`
- these items should be tracked as backlog GH issues rather than kept on the
  critical path to system testing

Phase rules:
- observability cleanup must treat subsystem meaning as subsystem-owned:
  observability may emit structured events, but it must not import subsystem
  types or reconstruct subsystem semantics centrally
- `team` and message-context fields are per-event payload, not injected logger
  state
- daemon observability infrastructure may own bootstrap, sink setup, emit,
  query, follow, and health only; it must not become a backdoor coordination
  layer
- Phase V must leave explicit code-removal targets, not only abstract design
  goals; the old central mapping helpers and wrapper paths should be deleted or
  collapsed once subsystem-owned emission lands
- system-testing-critical work stays on the Phase V execution line; other
  post-mortem hardening items move to backlog unless they block testing

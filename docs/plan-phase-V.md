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

Rationale:
- `RULE-002` / `ARCH-PU-002` around
  `crates/atm-daemon/src/daemon_observability.rs` `emit_runtime_event`
  drive the observability boundary/model, migration, and cleanup sequence in
  `V.1` through `V.3`
- `QA-U-002` plus `RBP-PU-001` / `RBP-PU-002` drive `V.4` recovery context
  hardening because system testing needs actionable daemon/runtime failures

Authoritative sprint sequence:
- `docs/phase-V/sprint-V1.md`
- `docs/phase-V/sprint-V2.md`
- `docs/phase-V/sprint-V3.md`
- `docs/phase-V/sprint-V4.md`

Sprint summary:
- `V.1` observability boundary and event model
- `V.2` observability migration into subsystems
- `V.3` observability removal and streamlining
- `V.4` recovery context hardening

Deferred backlog items:
- runtime test isolation lint
  - source finding: `FTQ-U9-001`
- workspace `#[path]` lint
- sprint-close hygiene gate
  - source findings: `ATM-QA-PU-001` through `ATM-QA-PU-005`
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
- system-testing-critical work stays on the Phase V execution line; other
  post-mortem hardening items move to backlog unless they block testing

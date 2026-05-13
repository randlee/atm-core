# Phase V — Daemon Hardening And Boundary Cleanup

Goal:
- close the daemon hardening follow-on work identified in the Phase U
  post-mortem before daemon internals are treated as settled
- add hard lint gates for runtime test seams and production cross-crate
  `#[path]` imports
- tighten daemon-unavailable recovery contracts and sprint-close hygiene
- close `ARCH-PU-002` by redesigning daemon observability as a bottom-of-stack
  sink boundary rather than a daemon-wide event reconstruction layer

Planning branch:
- `feature/daemon-hardening-plan`

Expected execution shape:
- `V.1` and `V.2` build hard lint gates on top of the existing lint-framework
  direction from `arch-inj` on `feature/pQ-lint-tools`
- `V.3` and `V.4` convert recurring release-gate findings into explicit
  checklist or linted requirements
- `V.5` closes the carried-forward daemon observability boundary issue and must
  delete or streamline obsolete mapping code rather than preserve it

Authoritative sprint sequence:
- `docs/phase-V/sprint-V1.md`
- `docs/phase-V/sprint-V2.md`
- `docs/phase-V/sprint-V3.md`
- `docs/phase-V/sprint-V4.md`
- `docs/phase-V/sprint-V5.md`

Sprint summary:
- `V.1` runtime test isolation lint
  - forbid global mutable test seams in runtime or transport code
- `V.2` workspace `#[path]` lint
  - forbid production cross-crate `#[path]` imports
- `V.3` recovery context hardening
  - require daemon-unavailable recovery guidance and consistent
    `.with_recovery()` coverage on the daemon/client/runtime path
- `V.4` sprint-close hygiene gate
  - require doc status, plan index, and sprint-close bookkeeping before QA
    handoff
- `V.5` daemon observability boundary cleanup
  - close `ARCH-PU-002`
  - keep observability at the bottom of the stack
  - remove central daemon event reconstruction
  - move subsystem event semantics to the owning subsystem through an injected
    thin logging trait

Phase rules:
- no Phase V sprint may preserve a known-bad runtime seam or boundary shortcut
  as an indefinite compatibility path
- lint sprints should prefer hard failure for production-bound violations over
  documentation-only warnings
- observability cleanup must treat subsystem meaning as subsystem-owned:
  observability may emit structured events, but it must not import subsystem
  types or reconstruct subsystem semantics centrally
- `team` and message-context fields are per-event payload, not injected logger
  state
- daemon observability infrastructure may own bootstrap, sink setup, emit,
  query, follow, and health only; it must not become a backdoor coordination
  layer

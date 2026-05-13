# Phase W — Production Readiness Follow-Up

Goal:
- close the four production-readiness gaps identified in `TASK-1477` before
  broader system testing depends on the new daemon/runtime line
- verify and restore the existing ATM error-reporting contract for critical
  daemon, command, and SQLite failures:
  - concise operator-facing ATM command failure output
  - deeper diagnostic detail available through `atm doctor`
- remove silent observability loss in daemon subsystems so incident evidence is
  not discarded when the sink is degraded

Planning branch:
- `feature/observability-findings-planning`

Critical issue reporting contract:
- Phase W treats the following as critical issue classes that must already be
  identified explicitly and surfaced in both operator and diagnostic channels:
  - daemon startup / daemon connect / daemon publish failure
  - ATM command execution failure on the daemon path, especially
    `atm send`, `atm read`, `atm ack`, `atm clear`, and `atm list`
  - SQLite writer / queue / reply / WAL / reader-budget failure
  - observability sink degradation severe enough to hide subsystem events
- operator-facing ATM commands must return concise failure output that names the
  failing surface and next action
- `atm doctor` must expose the richer diagnostic explanation, degraded-health
  evidence, and any retained observability trail for the same issue class
- Phase W is a no-regression audit for this contract:
  it closes places where the implemented daemon/runtime line drifted from the
  existing expectation that CLI and doctor both report critical failures
- shared reporting rule:
  - subsystem semantics stay local
  - logging emission, retained observability output, and doctor-facing
    reporting stay on shared paths
  - Phase W must not create separate per-subsystem reporting stacks

Execution shape:
- `W.1` removes daemon-side silent `emit()` discards and defines the fallback
  rule for sink degradation
- `W.2` adds daemon-client connection and auto-start traceability so daemon
  startup/connect failures are diagnosable
- `W.3` adds SQLite subsystem observability for writer queue, reply timeout,
  WAL lifecycle, and reader-budget exhaustion
- `W.4` closes the remaining peer replay recovery-text holes

Execution sequence:
- `W.1` must land first because the sink-failure rule affects every later
  observability change
- `W.2` and `W.3` both depend on `W.1`
- `W.4` can land in parallel with late `W.3` work if the replay-recovery
  branches remain isolated

Critical path rationale:
- without `W.1`, subsystem event loss is still silent
- without `W.2`, daemon-start/connect failures still collapse into an end
  error without enough attempt-level evidence for system testing
- without `W.3`, SQLite failures remain under-observed even though SQLite is
  the durable-state owner
- `W.4` is narrower, but it closes the remaining actionable recovery gaps in
  peer replay persistence and prevents ambiguous retry behavior

Authoritative sprint sequence:
- `docs/phase-W/sprint-W1.md`
- `docs/phase-W/sprint-W2.md`
- `docs/phase-W/sprint-W3.md`
- `docs/phase-W/sprint-W4.md`

Deliverables:
- `docs/phase-W/sprint-W1.md` — daemon `emit()` silent discard fix plan
- `docs/phase-W/sprint-W2.md` — daemon-client traceability plan
- `docs/phase-W/sprint-W3.md` — SQLite observability plan
- `docs/phase-W/sprint-W4.md` — peer replay recovery-text plan

Cross-sprint dependencies:
- `W.2` and `W.3` must both define how new signals map to:
  - concise ATM CLI failure output
  - detailed `atm doctor` findings or degraded-health reporting
- `W.3` may require a thin observability boundary extension into
  `atm-rusqlite`; the sprint doc must keep that extension bottom-of-stack and
  must not reintroduce daemon-owned semantic reconstruction
- `W.4` must reuse the Phase V recovery-text rules in
  `docs/atm-daemon/recovery-text-rules.md`
- every sprint must preserve one shared observability/reporting path:
  - shared observability trait for event emission
  - shared ATM CLI error surface for concise operator failures
  - shared `atm doctor` / runtime-health path for deeper diagnostics

Current path inventory requirement:
- every Phase W sprint doc must itemize the current log/error paths it will
  change
- path inventories must name concrete files and current functions or branch
  sites, not only modules
- no separate discovery or planning sprint is part of Phase W; the sprint docs
  themselves are the implementation-ready path inventories

Out of scope for Phase W:
- unrelated daemon redesign work
- broad new ATM product features
- replacing the current observability stack

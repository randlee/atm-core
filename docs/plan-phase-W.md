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

Phase scope note:
- Phase W is implementation planning only.
- It is not a discovery line and not a runtime-proof line by itself.
- Sprint docs must be detailed enough that implementation agents can execute
  them without adding a separate planning sprint.

Planning branch:
- `feature/observability-findings-planning`

Base branch:
- `origin/develop`

Integration branch:
- `integrate/phase-W`

Predecessor gate:
- all Phase `V` implementation branches are merged and validated on
  `integrate/phase-V` before `integrate/phase-W` starts taking implementation
  work

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
  - if duplicate interface-specific error/reporting paths already exist, Phase
    W should collapse them onto one shared implementation rather than preserve
    parallel code
- interface parity rule:
  - same critical failures must preserve the same ATM error codes and the same
    recovery intent across:
    - same-host ATM CLI
    - same-host `atm-graft` host flows
    - cross-daemon socket / peer transport flows
  - doctor/runtime-health diagnostics remain one shared diagnostic surface even
    when the failing interface differs

Shared implementation boundary:
- Phase W must converge the touched failure classes onto shared implementations
  rather than preserve parallel interface-specific handling.
- Shared code paths that should be reused or tightened when the touched failure
  class already exists there:
  - ATM error construction in `crates/atm-core/src/error.rs`
  - protocol-envelope mapping in `crates/atm-core/src/protocol.rs`
  - shared doctor engine in `crates/atm-core/src/doctor/mod.rs`
  - same-host daemon bootstrap/connect logic in
    `crates/atm-daemon-client/src/lib.rs`
  - CLI-facing command bootstrap/reporting in `crates/atm/src/composition.rs`
  - doctor command/output entrypoints in:
    - `crates/atm/src/commands/doctor.rs`
    - `crates/atm/src/output.rs`
  - doctor/runtime-health projection in:
    - `crates/atm-daemon/src/runtime_health.rs`
    - `crates/atm-daemon/src/runtime_status_cache.rs`
- Forbidden outcomes:
  - a new interface-specific error taxonomy
  - separate doctor reporting logic per participant
  - duplicate per-interface string formatting for the same failure class when a
    shared ATM error or protocol-envelope path can own it once

Shared observability trait decision:
- Phase `W` keeps `atm_core::observability::ObservabilityPort` as the one
  shared observability boundary for CLI, daemon, graft, and doctor-facing
  paths.
- Dispatch model:
  - object-safe trait
  - `dyn ObservabilityPort` / `Arc<dyn ObservabilityPort>` style runtime
    dispatch remains the shared model
  - Phase `W` must not fork into generic per-subsystem logger traits
- Sealing/open decision:
  - keep the existing `crate::boundary::sealed::Sealed` supertrait model in
    `atm-core`
  - Phase `W` does not widen the implementation surface or change the sealing
    decision
  - any sealing redesign is out of scope and would require ADR review

Interface parity matrix:
- same-host CLI:
  - `crates/atm/src/composition.rs`
  - `crates/atm-daemon-client/src/lib.rs`
- same-host graft host:
  - `crates/atm-graft/src/lib.rs`
  - `crates/atm-graft/src/runtime.rs`
  - `crates/atm-graft/src/transport.rs`
- cross-daemon socket / peer transport:
  - `crates/atm-daemon/src/peer_transport.rs`
- shared error-envelope / doctor surfaces:
  - `crates/atm-core/src/error.rs`
  - `crates/atm-core/src/protocol.rs`
  - `crates/atm-core/src/doctor/mod.rs`
  - `crates/atm/src/commands/doctor.rs`
  - `crates/atm/src/output.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`

Sprint ownership map:
- `W.1` owns daemon-side observability sink failure behavior and doctor/runtime
  degradation signaling for lost daemon subsystem events.
- `W.2` owns same-host interface parity and consolidation across:
  - `atm`
  - `atm-daemon-client`
  - `atm-graft`
- `W.3` owns SQLite-backed failure signaling, doctor projection, and protocol
  envelope parity for non-CLI consumers.
- `W.4` owns cross-daemon peer replay recovery text and peer-side parity
  through the shared protocol envelope.
- `W.5` owns doctor projection of same-host bootstrap traceability after `W.2`
  established the emitted daemon connect / launch-gate / auto-start trail.

Critical failure ownership matrix:
- daemon startup / connect / publish failure:
  - sprint owner: `W.2` for emission and interface parity, `W.5` for shared
    `atm doctor` projection
  - shared paths:
    - `crates/atm-daemon-client/src/lib.rs`
    - `crates/atm/src/composition.rs`
    - `crates/atm-core/src/error.rs`
    - `crates/atm-core/src/doctor/mod.rs`
    - `crates/atm/src/commands/doctor.rs`
    - `crates/atm/src/output.rs`
    - `crates/atm-daemon/src/runtime_health.rs`
- ATM command failure on same-host daemon path:
  - sprint owner: `W.2` for concise CLI parity, `W.5` where bootstrap failure
    evidence must be projected through the shared doctor surface
  - shared paths:
    - `crates/atm/src/composition.rs`
    - `crates/atm-daemon-client/src/lib.rs`
    - `crates/atm-core/src/error.rs`
    - `crates/atm-core/src/doctor/mod.rs`
    - `crates/atm/src/commands/doctor.rs`
    - `crates/atm/src/output.rs`
- SQLite writer / queue / reply / WAL / reader-budget failure:
  - sprint owner: `W.3`
  - shared paths:
    - `crates/atm-rusqlite/src/writer/mod.rs`
    - `crates/atm-rusqlite/src/shared_db.rs`
    - `crates/atm-rusqlite/src/lib.rs`
    - `crates/atm-core/src/error.rs`
    - `crates/atm-core/src/protocol.rs`
    - `crates/atm-core/src/doctor/mod.rs`
    - `crates/atm-daemon/src/runtime_health.rs`
- observability sink degradation:
  - sprint owner: `W.1`
  - shared paths:
    - `crates/atm-daemon/src/daemon_observability.rs`
    - `crates/atm-daemon/src/daemon_runtime_observability.rs`
    - `crates/atm-core/src/doctor/mod.rs`
    - `crates/atm-daemon/src/runtime_health.rs`
    - `crates/atm-daemon/src/runtime_status_cache.rs`
- remote delivery outcome unknown / replay persistence failure:
  - sprint owner: `W.4`
  - shared paths:
    - `crates/atm-daemon/src/peer_transport.rs`
    - `crates/atm-core/src/error.rs`
    - `crates/atm-core/src/protocol.rs`
    - `crates/atm-core/src/doctor/mod.rs`
    - `crates/atm-daemon/src/runtime_health.rs`

Shared ATM error inventory:
- `W.1` sink degradation uses existing ATM codes:
  - `ATM_OBSERVABILITY_EMIT_FAILED`
  - `ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED`
  - `ATM_OBSERVABILITY_HEALTH_FAILED`
- `W.2` same-host daemon bootstrap/connect uses existing ATM codes:
  - `ATM_DAEMON_UNAVAILABLE`
  - `ATM_DAEMON_AUTO_START_FAILED`
  - `ATM_DAEMON_LAUNCH_GATE_REJECTED`
  - `ATM_DAEMON_LIFECYCLE_WEDGE` when the current shared error surface already
    routes a lifecycle wedge rather than a generic unavailable failure
- `W.3` SQLite-backed command/runtime failures reuse existing ATM codes:
  - `ATM_DAEMON_UNAVAILABLE` for queue, reply, WAL, budget, and assembly
    failures that currently project through daemon/runtime availability
  - `ATM_DAEMON_LIFECYCLE_WEDGE` only where the existing shared error path
    already promotes the failure to a lifecycle wedge
- `W.4` remote replay persistence uses existing ATM codes:
  - `ATM_REMOTE_OUTCOME_UNKNOWN` for final operator-facing send failures where
    delivery outcome cannot be proven
  - `ATM_DAEMON_UNAVAILABLE` for lower-layer persistence prerequisites that are
    wrapped into the final remote-outcome-unknown error
- Phase `W` default decision:
  - no new `AtmErrorKind` variants are planned
  - no new `AtmErrorCode` variants are planned unless implementation proves an
    existing shared code cannot express the failure class without ambiguity
  - if that proof appears, the owning sprint must update this inventory and its
    own sprint-local error table in the same change

Execution shape:
- `W.1` removes daemon-side silent `emit()` discards and defines the fallback
  rule for sink degradation
- `W.2` adds daemon-client connection and auto-start traceability so daemon
  startup/connect failures are diagnosable
- `W.3` adds SQLite subsystem observability for writer queue, reply timeout,
  WAL lifecycle, and reader-budget exhaustion
- `W.4` closes the remaining peer replay recovery-text holes
- `W.5` projects the `W.2` bootstrap trace trail through shared
  `DoctorReport` / `atm doctor` output so the same evidence is visible without
  retained-log inspection
- `W.6` closes the remaining SQLite error-contract and typed daemon-event gaps
  discovered during the Phase `W` design review
- `W.7` closes the Phase `W` carry-forward triage loop and updates the merged
  sprint/status record required by the phase closeout gate

Execution sequence:
- `W.1` must land first because the sink-failure rule affects every later
  observability change
- `W.2` and `W.3` both depend on `W.1`
- `W.4` can land in parallel with late `W.3` work if the replay-recovery
  branches remain isolated
- `W.5` depends on `W.2`
- `W.6` depends on `W.3` and must merge-forward any shared doctor/runtime
  projection changes from `W.5` before push when those lines touch the same
  failure family

Critical path rationale:
- without `W.1`, subsystem event loss is still silent
- without `W.2`, daemon-start/connect failures still collapse into an end
  error without enough attempt-level evidence for system testing
- DESIGN-001-W:
  - `W.2` restored the emitted bootstrap trail, but that evidence still stayed
    inside observability records rather than the shared doctor surface
  - `W.5` exists because system testing needs the same daemon
    connect/launch/publish trail available from `atm doctor`, not only from
    retained-log inspection
- without `W.3`, SQLite failures remain under-observed even though SQLite is
  the durable-state owner
- DESIGN-002/003/004-W:
  - `W.6` exists because Phase `W` still was not complete until SQLite
    degradation projected through the right ATM warning code and daemon event
    metadata stayed typed through the retained observability path
- `W.4` is narrower, but it closes the remaining actionable recovery gaps in
  peer replay persistence and prevents ambiguous retry behavior

Authoritative sprint sequence:
- `docs/phase-W/sprint-W1.md`
- `docs/phase-W/sprint-W2.md`
- `docs/phase-W/sprint-W3.md`
- `docs/phase-W/sprint-W4.md`
- `docs/phase-W/sprint-W5.md`
- `docs/phase-W/sprint-W6.md`
- `docs/phase-W/sprint-W7.md`

Deliverables:
- `docs/phase-W/sprint-W1.md` — daemon `emit()` silent discard fix plan
- `docs/phase-W/sprint-W2.md` — daemon-client traceability plan
- `docs/phase-W/sprint-W3.md` — SQLite observability plan
- `docs/phase-W/sprint-W4.md` — peer replay recovery-text plan
- `docs/phase-W/sprint-W5.md` — doctor projection of bootstrap traceability
- `docs/phase-W/sprint-W6.md` — SQLite error-contract and typed daemon-event
  cleanup
- `docs/phase-W/sprint-W7.md` — final triage closeout and merged-status
  registry

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
- every sprint must preserve one shared error contract across interfaces:
  - no interface-specific replacement error taxonomy
  - no drift where CLI, graft, and peer transport describe the same failure
    class with incompatible ATM codes or contradictory recovery guidance
  - duplicate error-mapping or reporting code paths for the same failure class
    should be collapsed when the sprint touches them
- `W.4` is independently executable from `W.2` and `W.3` at the code-scope
  level; it depends only on the existing shared protocol/error contract and on
  ordinary merge-forward discipline.
- `W.1` does not own daemon-client tracing or CLI-side path narration; those
  same-host path gaps are owned by `W.2`.
- `W.5` depends on `W.2`; it must project the bootstrap evidence that `W.2`
  emits rather than invent a second same-host trace taxonomy.
- `W.6` depends on `W.3`; it may tighten SQLite degradation projection and
  daemon-event typing, but it must not fork a second SQLite warning taxonomy
  or bypass the shared doctor/runtime-health reporting path.
- if `W.3` and `W.4` run in parallel, `crates/atm-core/src/protocol.rs`
  changes must be partitioned by failure family:
  - `W.3` owns SQLite-backed envelope parity
  - `W.4` owns peer replay / remote-delivery envelope parity
  - both branches must merge-forward before push if `protocol.rs` changed on
    the other line; no parallel fork of protocol error taxonomy is allowed
- if audit finds any critical-failure path not assignable to `W.1` through
  `W.4`, the plan must add the missing sprint rather than leave it implicit.

Current path inventory requirement:
- every Phase W sprint doc must itemize the current log/error paths it will
  change
- path inventories must name concrete files and current functions or branch
  sites, not only modules
- for every touched critical failure class, the sprint doc must also name the
  current shared CLI/doctor/error baseline that exists on `main` so
  implementation can verify no regression while collapsing duplicate paths
- no separate discovery or planning sprint is part of Phase W; the sprint docs
  themselves are the implementation-ready path inventories

Plan QA boundary:
- req-qa can fully validate this plan as a document set without running the
  daemon only if each sprint doc clearly separates:
  - document-auditable acceptance criteria
  - implementation/runtime validation that the future sprint must run

Phase closeout gate:
- Phase `W` is not complete until:
  - all critical failure classes above are implemented on `integrate/phase-W`
  - shared CLI / graft / peer ATM error-code parity is revalidated
  - `atm doctor` coverage for the touched failure classes is revalidated
  - no duplicate interface-specific reporting path remains for the touched
    failure classes
  - the final Phase `W` sprint docs and `docs/project-plan.md` status reflect
    the merged implementation state

Out of scope for Phase W:
- unrelated daemon redesign work
- broad new ATM product features
- replacing the current observability stack

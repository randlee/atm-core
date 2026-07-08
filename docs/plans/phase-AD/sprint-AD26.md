---
id: AD.26
title: Post-Send Boundary Wiring And Hook Accounting Repair
status: planned
branch: feature/pAD-s26-post-send-boundary-wiring-and-accounting
worktree: ../atm-core-worktrees/feature/pAD-s26-post-send-boundary-wiring-and-accounting
target: integrate/phase-AD
---

# Sprint AD.26 — Post-Send Boundary Wiring And Hook Accounting Repair

## Goal

- make the accepted post-send boundary real on the production send/ack path,
  and fix mixed-success hook accounting so successful emission is never hidden
  by another matching hook failure

## Hard Dependencies

- `AD.25` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- review provenance:
  - ATM message `01KX1P4D0SEZXWW90VW2F7FF27` from `quality-mgr`,
    `2026-07-08`, subject `PHASE-AD-END-QA FINAL VERDICT`
  - ATM message `01KX1MTJE596JE8SC2766V0Q10` from `arch-ctm`,
    `2026-07-08`, subject `PHASE-AD-END-REVIEW complete`

`AD.25` is a functional dependency, not just merge order: this sprint cannot
wire mixed-success hook accounting correctly until the accepted override store
has explicit override/disable/clear semantics instead of the hidden empty-row
state.

## Exact Targets

- `boundaries/atm-core/post-send-hook-emitter.toml`
- `boundaries/atm-core/graft-post-send-port.toml`
- `.just/lint_boundaries.py`
- `.just/allowlists/scb_observability_allowlist.toml`
- `.just/fixtures/scb_observability_known_bad.rs`
- `crates/atm-core/src/boundary/mod.rs`
- `crates/atm-core/src/send/hook.rs`
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_sqlite_observer.rs`
- `crates/atm-daemon/src/test_observability.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm/src/commands/internal_nudge.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/observability.md`
- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`
- `docs/adr/ADR-020-rule001-observability-adapter-exception.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD26.md`

## Interfaces To Add Or Modify

This sprint makes the architectural call explicitly:

- keep `PostSendHookEmitter`
- keep `GraftPostSendPort`
- wire both as live runtime seams
- delete the `std::process::Command` subprocess bypass from the accepted
  send/ack path

ADR-019 already fixes the accepted architecture to one direct post-persist
emitter seam with receiver-specific handoff staying capability-specific, so
wiring the existing boundaries is the correct closure and retiring them would
contradict the accepted Phase AD design rather than repairing implementation
drift.

The accepted accounting shape after this sprint is:

```rust
pub struct HookExecutionSummary {
    matched_rules: usize,
    succeeded_rules: usize,
    failed_rules: usize,
}

impl HookExecutionSummary {
    pub fn new(
        matched_rules: usize,
        succeeded_rules: usize,
        failed_rules: usize,
    ) -> Result<Self, AtmError>;

    pub fn matched_rules(&self) -> usize;
    pub fn succeeded_rules(&self) -> usize;
    pub fn failed_rules(&self) -> usize;
}

pub enum PostSendEmissionPath {
    ExternalHook,
    LocalTmux,
    GraftPort,
}

pub enum PostSendBuiltInTarget {
    LocalTmux(LocalTmuxNudgeTarget),
    Graft(GraftNudgeTarget),
}

pub struct BuiltInPostSendDispatch {
    pub event: PostSendHookEvent,
    pub target: PostSendBuiltInTarget,
}

pub trait GraftPostSendPort: sealed::Sealed + Send + Sync {
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError>;
}

pub trait PostSendHookEmitter: sealed::Sealed + Send + Sync {
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError>;
}

pub enum PostSendEmissionOutcome {
    NoCapability,
    Delivered {
        path: PostSendEmissionPath,
        hook_summary: HookExecutionSummary,
    },
    Failed {
        hook_summary: HookExecutionSummary,
        warning: WarningEntry,
    },
}
```

Required runtime meaning after this sprint:

- caller-owned send/ack logic stays responsible for:
  - deciding whether the recipient exposes post-send capability
  - matching and executing external hook rules in config order
  - deciding whether built-in fallback is legal after external-hook matching
  - constructing the concrete built-in recipient target before invoking
    `PostSendHookEmitter`
  - constructing sender-visible warnings and appending log records from the
    typed result
- graft-backed delivery attempts call `graft_port.deliver_post_send(...)`
  directly on the accepted send/ack runtime path after the caller-owned send
  logic selects a graft-backed built-in target
- local tmux-backed delivery stays behind the accepted emitter seam and does
  not use `std::process::Command` subprocess spawn from
  `crates/atm-core/src/send/hook.rs`
- matching external hook rules still execute in config order
- any successful matching external hook counts as real emission
- built-in fallback is attempted only when zero external hook rules matched
- sender-visible warning is appended only when a post-send-capable recipient
  saw total emission failure
- partial failures keep warnings and logs, but do not erase a successful
  emission outcome
- notification log append occurs on any real successful emission, even when a
  sibling matching hook also failed
- `HookExecutionSummary::new(...)` must reject any state where
  `succeeded_rules + failed_rules > matched_rules`; raw field mutation is not
  an accepted implementation path
- `PostSendEmissionOutcome::Failed.warning` must reuse one of the stable
  `AD.6` warning/error codes rather than inventing a new generic failure code:
  - `ATM_POST_SEND_PANE_MISSING`
  - `ATM_POST_SEND_TMUX_SEND_FAILED`
  - `ATM_POST_SEND_GRAFT_UNAVAILABLE`
  - `ATM_POST_SEND_ADVISORY_DELIVERY_FAILED`
- the deferred `AD18/ARCH-004` scope ruling lands here for the dual
  `lib.rs` + `main.rs` `atm-daemon` crate as a library-internal adapter
  exception, not a binary-internal one:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs` is a real library
    module declared from `lib.rs` and publicly re-exported; this sprint must
    describe it honestly as the sanctioned library-internal adapter module
  - that module becomes the only sanctioned non-`main.rs` location allowed to
    import `sc_observability_types::{ActionName, OutcomeLabel}` directly
  - the concrete achievable mechanism is:
    - export crate-visible aliases or constructor helpers from
      `daemon_runtime_observability.rs`, for example
      `pub(crate) type DaemonActionName = sc_observability_types::ActionName`
      and `pub(crate) type DaemonOutcomeLabel = ...`
    - make `runtime_sqlite_observer.rs` and `test_observability.rs` consume
      those crate-visible daemon aliases/helpers instead of importing
      `sc_observability_types` directly
  - the sign-off record for this sanctioned library-internal adapter exception
    must be restated in `docs/atm-core/boundaries.md`,
    `docs/plans/phase-AD/readiness.md`, and a dedicated ADR
    `docs/adr/ADR-020-rule001-observability-adapter-exception.md`
  - the sprint must also wire this exception into `.just/lint_boundaries.py`
    using the existing allowlist pattern extended with one explicit module-root
    sentinel such as `symbol = "__module__"` so the sanctioned adapter file is
    mechanically allowlisted while any new direct import elsewhere still fails
- `LocalTmuxNudgeTarget` reuses the roster-backed pane-routing target shape
  already accepted in `AD.22`; it must not fall back to `.atm.toml` pane
  lookup
- `GraftNudgeTarget` is intentionally thin and identifies only the receiver the
  graft sink must wake; it must not grow session-registration, advisory-stream,
  or queue-drain fields

`atm internal-nudge` may remain temporarily as a thin renderer/delivery helper,
but it is no longer allowed to be the production boundary bypass on the send
path. `AD.27` owns the remaining extraction cleanup around that helper.

`ADR-019` interim exception handling for this sprint is explicit:

- `AD.26` closes the dead-seam problem by making `PostSendHookEmitter` and
  `GraftPostSendPort` live on the production path
- `AD.26` does **not** claim to close the separate
  "override lookup upstream of `PostSendHookEmitter`" clause from `ADR-019`
- that single remaining exception is tracked as `ADR-019-EXC-AD26-001` and
  must be closed by `AD.27`
- the separate `RULE-001` library-internal adapter exception is governed by
  `ADR-020` and must not be described as binary-internal anywhere on the
  accepted line

## Paths To Delete

- unused `_graft_port` threading with no live call to
  `.deliver_post_send(...)`
- any `std::process::Command`-based post-send delivery bypass on the accepted
  send/ack runtime path
- boundary TOMLs or readiness criteria that claim a live emitter seam while the
  implementation still bypasses it
- mixed-success accounting that treats “matched with one success and one
  failure” as no emission
- direct `sc_observability_types::{ActionName, OutcomeLabel}` imports anywhere
  under `crates/atm-daemon/src/` except:
  - `crates/atm-daemon/src/main.rs`
  - `crates/atm-daemon/src/daemon_runtime_observability.rs`
- manual-only QA grep gates with no matching `.just/lint_boundaries.py`
  enforcement for the sanctioned adapter exception

## Deliverables

- `PostSendHookEmitter` has at least one real implementation and one real
  production call site on the accepted send/ack path
- graft-backed delivery goes through `GraftPostSendPort`
- mixed-success hook execution is accounted as matched/succeeded/failed
  distinctly
- notification logging and sender warnings reflect real delivery outcome rather
  than the previous all-or-nothing warning vector shortcut
- boundary TOMLs, boundary inventory docs, readiness criteria, and runtime code
  all describe the same mechanism
- the accepted runtime observability helpers close `RULE-001` by routing
  `ActionName` / `OutcomeLabel` through `DaemonRuntimeObservability`
  everywhere under `crates/atm-daemon/src/` except the explicitly sanctioned
  `daemon_runtime_observability.rs` encapsulation seam
- `ADR-020` records the scope, rationale, review conditions, and CI
  enforcement requirements for this library-internal adapter exception
- `.just/lint_boundaries.py`, `.just/allowlists/scb_observability_allowlist.toml`,
  and `.just/fixtures/scb_observability_known_bad.rs` mechanically enforce the
  exception in CI instead of relying only on a manual review-time grep

## This Sprint Does Not Close

- explicit set/disable/clear lifecycle for template overrides
- upstream movement of template resolution out of `atm internal-nudge`
- the `atm-graft` timing race
- the phase-end smoke/service-hardening lane
- `ADR-019-EXC-AD26-001`, the temporary allowance that built-in override
  lookup is still extracted by `AD.27`

## Acceptance Criteria

- `rg 'emit_post_send\\(' crates --glob '!**/tests.rs'` shows a live
  production call path for `PostSendHookEmitter` itself, not only a trait
  definition or a test-only caller
- `rg 'deliver_post_send\\(' crates` shows a live production call path, not
  only trait definition or tests
- `rg 'std::process::Command' crates/atm-core/src/send/hook.rs` returns no
  accepted send-path subprocess bypass
- targeted validation proves `HookExecutionSummary::new(...)` rejects invalid
  accounting states where successes plus failures exceed matches
- targeted tests prove:
  - matched hook success + sibling hook failure still logs successful emission
  - total external-hook failure returns sender-visible warning
  - zero matching hooks still trigger the built-in path
  - graft-backed delivery goes through the graft port rather than subprocess
    bypass
- `rg -n 'sc_observability_types::(ActionName|OutcomeLabel)' crates/atm-daemon/src --glob '!main.rs'`
  returns matches only in
  `crates/atm-daemon/src/daemon_runtime_observability.rs`
- targeted daemon-observability validation proves no other file under
  `crates/atm-daemon/src/` imports
  `sc_observability_types::{ActionName, OutcomeLabel}` directly
- `python3 .just/lint_boundaries.py` fails on
  `.just/fixtures/scb_observability_known_bad.rs` and accepts only the
  `daemon_runtime_observability.rs` module-root allowlist entry declared in
  `.just/allowlists/scb_observability_allowlist.toml`
- `boundaries/atm-core/post-send-hook-emitter.toml`,
  `boundaries/atm-core/graft-post-send-port.toml`,
  `docs/atm-core/boundaries.md`,
  `docs/adr/ADR-020-rule001-observability-adapter-exception.md`,
  `docs/plans/phase-AD/plan-phase-AD.md`, and
  `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md` all match
  the accepted live mechanism

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted post-send accounting regression tests
- targeted graft-port delivery regression tests
- `git diff --check`

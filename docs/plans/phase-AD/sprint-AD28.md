---
id: AD.28
title: atm-graft Host-Nudge Deadline Race Hardening
status: complete
branch: feature/pAD-s28-atm-graft-timing-independent
worktree: ../atm-core-worktrees/feature/pAD-s28-atm-graft-timing-independent
target: integrate/phase-AD
---

# Sprint AD.28 — `atm-graft` Host-Nudge Deadline Race Hardening

## Goal

- remove the real `atm-graft` host-nudge timing race by making test readiness
  deterministic instead of relying on a shortened `#[cfg(test)]` deadline

## Hard Dependencies

- `AD.17` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `FTQ-AD-END-001` from ATM message `01KX1P4D0SEZXWW90VW2F7FF27`
  (`quality-mgr`, `2026-07-08`, subject `PHASE-AD-END-QA FINAL VERDICT`)

## Exact Targets

- `crates/atm-graft/src/runtime.rs`
- `crates/atm-graft/src/nudge_sink.rs`
- `docs/atm-graft/requirements.md`
- `docs/atm-graft/architecture.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD28.md`

## Interfaces To Add Or Modify

This sprint takes the deterministic-readiness option, not the “just make the
test timeout larger” option.

The accepted test helper shape after this sprint is:

```rust
pub(crate) struct TestReceiverReadyLatch {
    ready_tx: std::sync::mpsc::SyncSender<()>,
    ready_rx: std::sync::mpsc::Receiver<()>,
}

impl TestReceiverReadyLatch {
    pub(crate) fn signal_listening(&self) -> Result<(), AtmError>;
    pub(crate) fn wait_until_listening(&self, timeout: Duration) -> Result<(), AtmError>;
}
```

The accepted implementation may keep the latch wait-side in the test harness
and clone only the signal sender into the receiver-loop context, as long as the
receiver loop signals readiness exactly once after the listener bind succeeds.

Required runtime/test meaning after this sprint:

- production and test host-nudge injection use the same bounded deadline value
  unless a narrower test deadline is independently justified in the sprint doc
  and proven stable
- the test path does not start timing delivery success until the receiver side
  has signaled readiness explicitly
- race closure is proven with repeated-load regression coverage, not one
  best-effort happy-path test run
- any retained timeout test fails fast with typed error output; no test may
  block a CI runner for an unbounded or scheduler-luck duration

## Paths To Delete

- the special `#[cfg(test)]` `50ms` host-nudge deadline divergence
- scheduler-luck test behavior that depends on the receiver thread winning the
  race before injection begins
- any test that “passes” only because the injected timeout is so short that the
  failure path becomes common under load

## Deliverables

- `HOST_NUDGE_INJECTION_DEADLINE` or equivalent no longer diverges to a
  scheduler-racy `50ms` test-only value
- test readiness is explicit and deterministic before host nudge injection is
  asserted
- targeted repeated-load coverage proves the race is closed
- docs describe the accepted deterministic readiness model instead of implying
  timeout luck is acceptable

## This Sprint Does Not Close

- post-send boundary wiring
- template override lifecycle/reset
- upstream template resolution extraction
- end-to-end smoke/service-hardening closeout

## Acceptance Criteria

- targeted repeated-run coverage exercises the repaired path enough times to
  prove scheduler luck is no longer the deciding mechanism; the sprint closes
  only with one targeted test or harness that runs at least `100` consecutive
  injections on the repaired path in one invocation
- no retained test uses the old test-only `50ms` deadline divergence
- failures on the repaired path are bounded, typed, and fail fast
- docs no longer describe the old timeout shortcut as accepted behavior

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted repeated-run `atm-graft` regression coverage for the former host
  nudge race
- `git diff --check`

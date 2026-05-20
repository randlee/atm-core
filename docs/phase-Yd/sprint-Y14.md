---
id: Y.14
title: Recovered Claude Logical-Message-Set Closure
status: planned
branch: feature/pYd-s14-recovered-claude-logical-message-set-closure
worktree: ../atm-core-worktrees/feature/pYd-s14-recovered-claude-logical-message-set-closure
target: integrate/phase-Y
---

# Sprint Y.14 — Recovered Claude Logical-Message-Set Closure

## Goal

- close the remaining recovered Claude behavioral correctness blocker before
  the `Phase Y` line is proposed for `develop`

## Hard Dependencies

- `docs/phase-Y/issues.md`
- `docs/phase-Yd/plan-phase-Yd.md`
- `docs/phase-Yd/readiness.md`
- `docs/phase-Yc/plan-phase-Yc.md`
- `docs/adr/INDEX.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `boundaries/atm-core/inbox-export.toml`
- `docs/phase-Y/delivery-state-machines.md`
- `docs/testing-guidelines.md`
- the authoritative implementation baseline remains `integrate/phase-Y`

## Exact Targets

- `crates/atm-core/src/delivery_execution.rs`
- any directly supporting `atm-core` files required to close the recovered
  Claude message-set contract cleanly
- `docs/phase-Y/issues.md`
- `docs/phase-Yd/readiness.md`
- `docs/project-plan.md`

## Deliverables

- recovered Claude SQLite-failure delivery either emits the full logical
  message set or fails hard
- no partial outward success may survive on the recovered Claude path
- the blocker inventory and readiness record explicitly record the `Y.14`
  closure result

## Relationship To Phase Yc

`Y.12` already defined and first proved the recovered Claude
logical-message-set closure invariant on the focused `Yc` line.

`Y.14` does not silently reopen that design. It re-proves the same invariant on
the final accepted `Phase Y` candidate line after later accepted line-state
changes. `Y.14` is therefore a final-candidate behavioral re-proof sprint, not
a new semantic redesign of the recovered Claude path.

## Required Work

- close the recovered Claude logical-message-set blocker recorded in
  `docs/phase-Y/issues.md`
- update the blocker inventory and readiness record to reflect the closure
  state
- keep `Phase Z` blocked while the later `Y.15` through `Y.18` closures remain
  open

## Explicit Code Samples

```rust
pub enum DeliveryPlanDisposition {
    SqliteFailedRecovered {
        logical_message_set: Vec<CompatInboxMessage>,
        recovery_error: AtmError,
    },
}
```

```rust
match disposition {
    DeliveryPlanDisposition::SqliteFailedRecovered {
        logical_message_set,
        recovery_error,
    } => {
        inbox_export.append_message_set(&logical_message_set)?;
        return Err(recovery_error);
    }
}
```

```rust
// Required Y.14 behavior:
// either the full logical message set is emitted or the path fails hard.
// Forbidden behavior:
// emit message[1], fail on message[2], and still report outward success.
```

## This Sprint Does Not Close

- production `NotificationSink` boundary closure
- daemon retained-runtime `NotificationSink` installation
- accepted phase-end fix candidate absorption
- final `develop`-gate authorization
- notification shutdown determinism hardening beyond the bounded accepted
  production contract

## Acceptance Criteria

- the recovered Claude blocker assigned to `Y.14` in `docs/phase-Y/issues.md`
  is closed or explicitly reclassified with documented rationale
- `rg -n "if disposition == DeliveryPlanDisposition::SqliteFailedRecovered \\{[[:space:]]*break;" crates/atm-core/src/delivery_execution.rs`
  returns no matches
- `rg -n "append_claude_inbox_message\\(inbox_path, recipient, &message\\.envelope\\)" crates/atm-core/src/delivery_execution.rs`
  no longer shows the recovered Claude path implemented as a one-message loop
- the recovered Claude path either materializes the full logical message set
  or fails hard; no partial outward success remains on the accepted line
- named tests prove all-or-nothing recovered Claude logical-message-set
  behavior on the final accepted line:
  - `sqlite_failure_for_claude_requires_full_logical_message_set_delivery`
  - `sqlite_failure_for_claude_does_not_emit_message1_without_message2`
- the final accepted `Phase Y` merge candidate is behavioral-clean for the
  recovered Claude scope
- `docs/phase-Yd/readiness.md` is updated with the `Y.14` closure result

## Required Validation

- `rg -n "if disposition == DeliveryPlanDisposition::SqliteFailedRecovered \\{[[:space:]]*break;" crates/atm-core/src/delivery_execution.rs`
- `rg -n "append_claude_inbox_message\\(inbox_path, recipient, &message\\.envelope\\)" crates/atm-core/src/delivery_execution.rs`
- `cargo fmt --all`
- `python3 .just/run_lint.py all`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`

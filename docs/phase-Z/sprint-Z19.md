---
id: Z.19
title: Fast Smoke Happy-Path Execution
status: planned
branch: feature/pZ-s19-fast-smoke-happy-path-execution
worktree: ../atm-core-worktrees/feature/pZ-s19-fast-smoke-happy-path-execution
target: integrate/phase-Z
---

# Sprint Z.19 — Fast Smoke Happy-Path Execution

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.19
worktree: ../atm-core-worktrees/feature/pZ-s19-fast-smoke-happy-path-execution
branch: feature/pZ-s19-fast-smoke-happy-path-execution
status: planned
estimated_scope: medium
```

## Goal

- implement `just smoke fast`
- prove the clean-room happy path quickly and reliably
- make the fast retained-log evidence deterministic
- fix minor smoke-blocking issues in-sprint when they are small and localized

## Scope Summary

This sprint owns the quick end-to-end "everything basically works" lane. It
must prove both send modes plus read/ack/nudge behavior on a new disposable
baseline, then validate that the retained logs show the expected happy-path
events with no warnings or errors.

## Governing Requirements

- `REQ-CORE-ATM-JSON-001`
- `REQ-CORE-CLI-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RuntimeFactory`

## Prerequisites

- `Z.18` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `scripts/smoke/run.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/smoke/render_report.py`
- `templates/smoke-report/smoke-fast.md.j2`
- `Justfile`
- `reports/smoke/smoke-fast.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- `just smoke fast`
- fast-level latest snapshot report at `reports/smoke/smoke-fast.md`
- timestamped fast smoke markdown and JSON artifacts
- deterministic fast log-analysis gate
- clear fast-run root-cause output when the lane fails
- minor in-sprint fixes required to make the fast lane predictable

## Required Work

- create a new disposable DB/runtime baseline
- bring up the daemon
- create/setup the clean-room team baseline
- prove successful `doctor`
- prove successful `atm send` without `--requires-ack`
- prove successful `atm send` with `--requires-ack`
- prove successful `atm read`
- prove successful `atm ack`
- prove successful nudge-visible flow triggered by the successful
  `atm send --requires-ack` path
- require the fast report to show that the ack-required send completed, the
  recipient read the durable message, and the recipient ack was accepted
- shut the daemon down cleanly
- enable detailed debug/verbose logging for smoke-fast
- analyze retained logs and fail the run if expected lifecycle/send/read/ack/
  nudge events are missing
- analyze retained logs and fail the run on any warning or error output
- add missing log messages at the appropriate debug/verbose level when that is
  the only local blocker
- fix minor localized requirement or architecture violations when they are the
  only local blocker to predictable fast execution
- promote larger rework findings into `docs/phase-Z/smoke-findings-review.md`

## Explicit Code Samples

```text
just smoke fast
```

## Nudge-Visible Flow Definition

- trigger:
  - one successful `atm send --requires-ack ...`
- caller-visible proof:
  - the send command succeeds
  - the recipient can `atm read` the message
  - the recipient can `atm ack` the message successfully
- retained-log proof:
  - `FAST-LOG-001` must confirm the happy-path transition sequence contains
    `delivery_policy.new_message.primary_nudge`
  - the retained logs must also show the smoke-fast debug/verbose send/ack
    trail needed to correlate the nudge with the durable send/read/ack cycle
- failure rule:
  - if the send/read/ack commands succeed but the expected nudge event is
    missing from retained logs, the row fails and the missing log evidence is
    treated as a smoke finding

## This Sprint Does Not Close

- the default `just smoke` systemic lane
- the `just smoke thorough` full CLI lane
- major rework findings discovered during fast smoke execution

## Acceptance Criteria

- `just smoke fast` exists and executes the fast level
- the fast run explicitly reports `Z1-001`, `Z1-002`, `Z1-003`, `Z1-004`,
  `Z1-005`, `FAST-LOG-001`, and `FAST-LOG-002`
- the fast lane proves clean-room daemon bring-up and clean shutdown
- the fast lane proves `doctor`
- the fast lane proves `atm send` without `--requires-ack`
- the fast lane proves `atm send` with `--requires-ack`
- the fast lane proves `atm read`
- the fast lane proves `atm ack`
- the fast lane proves nudge-visible flow
- retained logs are analyzed by script and the fast run fails on missing
  expected events or any warning/error output
- the fast report is rendered to the tracked-latest and timestamped artifacts
- any remaining large issue is captured in
  `docs/phase-Z/smoke-findings-review.md`

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If the sprint starts absorbing the broader retained/admin/operator surface from
the normal lane or full CLI/error-path coverage from the thorough lane, stop
and keep that work in `Z.20` or `Z.21`.

## Production-Ready Expectation

Every listed `Z.19` deliverable is expected to land at a production-ready
level for deterministic fast smoke execution.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`
- `docs/phase-Z/smoke-findings-review.md`, when needed

## Risks And Watchouts

- do not let fast drift back into a startup-only check
- do not hide missing log messages behind manual interpretation
- keep detailed smoke logging out of the normal production log level defaults

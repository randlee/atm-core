# Phase S.5 — No-Flaky-Test Policy And Mechanical Guardrails

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.5"
status: planned
estimated_scope: M
```

## Goal

Close the remaining Phase S process gap by making the no-flaky-test contract
phase-wide and by defining the mechanical lint families that must prevent
hang-prone regression patterns from re-entering the same-host daemon line.

## Governing Requirements

- `REQ-P-TEST-001`
- `REQ-P-LINT-POSTMORTEM-001`
- `REQ-CORE-TEST-RUNTIME-001`
- `REQ-DAEMON-TEST-004`
- `REQ-DAEMON-PLATFORM-002`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Hard Dependencies

- `integrate/phase-S` contains the merged S.1 through S.4 implementation line
- the existing fixed-sleep, singleton, portability, and runtime-waits lint
  gates remain green before S.5 follow-on planning starts

## Required Work

1. Tighten the Phase S wording from "no fixed sleeps" to the stronger
   no-flaky-test and no-unbounded-wait policy.
1.1 Update the active Phase S plan and sprint docs so same-host daemon tests
    explicitly forbid:
   - unbounded waits
   - missed-wakeup-sensitive waits with no bounded fallback
   - panic-unsafe global/shared test hooks
   - retry-until-success loops with no state predicate
1.2 Carry the same contract into:
   - `docs/testing-guidelines.md`
   - `docs/cross-platform-guidelines.md`
   - top-level requirements and architecture docs

2. Define the mechanical anti-flake guardrail inventory.
2.1 Record which rules are feasible now in the default `just lint` path:
   - fixed-sleep test hygiene, with the current repository-local rule treated
     as the proving implementation for later `sc-lint` extraction
   - daemon-spawn helper rejection
   - production bare `Condvar::wait(...)`
   - production discarded `wait_timeout*` results
   - targeted same-host daemon test checks for cheap unbounded-wait syntax
2.2 Record which rules require deferred analyzer or rule-design work:
   - path-sensitive `JoinHandle::join()` safety
   - polling-loop terminate-state placement
   - panic-safe cleanup proof for global test hooks
   - bounded-wait result handling in test code

3. Name the intended lint ownership for each family.
3.1 Reusable Rust-aware analyzer rules remain `sc-lint-*` candidates.
3.2 ATM-specific repository policy checks remain `.just/` or `scripts/` lints
    until their rule shape is proven reusable.

4. Update the Phase S issue inventory so the remaining policy and guardrail
   gaps are explicit rather than implied by QA findings.

5. Record any ADR needed for the repository-wide no-flaky-test decision and
   enforcement partition.

6. Harden the Phase S triage process so canonical `.triage/<phase>/findings/*.ttl`
   records are committed to git at the correct workflow point.
6.1 Set the commit point at the team-lead triage batch stage:
   - after all parallel `qa-triage` agents finish writing `.ttl` files
   - after aggregation confirms the batch is complete
   - before any branch-scoped fix dispatch to `arch-ctm`
6.2 Update the relevant triage prompt/skill so the process explicitly forbids
    leaving phase findings untracked in the working tree until phase end.
6.3 Keep this track separate from the mailbox-read planning follow-up; the
    triage git-commit gap is an independent process-hardening item.

## Required Document Updates

- `docs/plan-phase-S.md`
- `docs/phase-S/issues.md`
- `docs/phase-S/sprint-S0.md`
- `docs/phase-S/sprint-S1.md`
- `docs/phase-S/sprint-S2.md`
- `docs/phase-S/sprint-S3.md`
- `docs/phase-S/sprint-S4.md`
- `docs/testing-guidelines.md`
- `docs/cross-platform-guidelines.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`
- `docs/adr/INDEX.md`
- `.claude/agents/qa-triage.md`
- `.claude/skills/triaging-findings/SKILL.md`

## Acceptance Criteria

- Phase S has one explicit phase-wide no-flaky-test contract rather than a
  narrow fixed-sleep-only rule
- the active docs state that a test which might hang is invalid even if it
  does not use `thread::sleep(...)`
- the active docs distinguish feasible-now lint families from deferred
  analyzer work
- the default development gate remains `just lint`, and the S.5 docs name
  which new anti-flake families belong there
- the mechanical guardrail plan names the intended implementation home for
  each rule family (`sc-lint-*` vs `.just/` / `scripts/`)
- the triage workflow names an explicit git-commit step for phase `.ttl`
  records before dev dispatch begins
- the chosen triage commit point prevents a repeat of the Phase S gap where
  findings remained untracked until manual intervention

## Required Validation

- `just lint`

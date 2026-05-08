# Phase R.19 — Postmortem Linter Backfill

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.19"
worktree: feature/pR-postmortem-linters
branch: feature/pR-postmortem-linters
status: planned
estimated_scope: M
```

## Goal

Convert the recurring mechanically-detectable Phase R defect families into
normal repository lint or CI gates.

This sprint is ATM-first by design: implementation lands on `atm-core`,
proves out on the live consumer repo, and only then migrates the reusable
subset into standalone `sc-lint`.

## Scope Summary

The Phase R postmortem identified five lint-worthy finding families:

1. Unix platform gating
2. duplicate semantic string literals in non-test Rust code
3. fixed `thread::sleep(...)` in ordinary test code
4. bare production `Condvar::wait(...)`
5. TTL triage-record consistency

This sprint does not merge those concerns into one tool. It records the
correct partition:
- reusable Rust/static-analysis rules go through the embedded `crates/sc-lint-*`
  surface on `atm-core`
- ATM-only policy and process checks stay in `.just/` or `scripts/`

## Governing Requirements

- `REQ-P-LINT-POSTMORTEM-001`
- `REQ-CORE-TEST-RUNTIME-001`
- `REQ-CORE-QA-RUNTIME-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`

## Governing Boundaries

- `BOUNDARY-ClientTransport`
- `BOUNDARY-AtmProtocol`

## Prerequisites

- the current `just lint` surface is green on `integrate/phase-R`
- the embedded `sc-lint` crates remain the active proving surface inside
  `atm-core`

## Hard Dependencies

- do not move rules into standalone `sc-lint` first; `atm-core` is the proving
  ground for this sprint
- every family must name its final implementation home before coding starts

## Non-Goals

- generalized extraction of every ATM-specific lint into standalone `sc-lint`
- replacing the existing identity-literal or portability lints wholesale when
  a narrower extension is sufficient
- inventing a new generic process-lint framework before the ATM-local needs are
  proven

## Partition

Reusable rules intended for later `sc-lint` migration:
- Family A — Unix platform gating
- Family D — bare production `Condvar::wait(...)`

ATM-local rules that remain on `atm-core` unless a later extraction is
explicitly approved:
- Family B — duplicate semantic string literals in non-test Rust code
- Family C — fixed-sleep test hygiene
- Family I — TTL triage-record consistency

## Sub-Tasks

1. Family A — Unix platform gating
   Development work:
   - extend `sc-portability` with an import-level rule for ungated
     `std::os::unix` usage
   - add a separate rule banning `cfg_attr(not(unix), allow(dead_code))` as a
     portability suppressor in production code
   Integration:
   - keep this on the existing `sc-portability` path so `just lint` continues
     to run it automatically
   Migration posture:
   - later migrate upstream to standalone `sc-lint` once the rule shape is
     stable on `atm-core`

2. Family B — duplicate semantic string literals
   Development work:
   - extend `.just/check_test_identity_literals.py` into a duplicate semantic
     string-literal gate for non-test Rust code
   - reject raw `"team-lead"` literals outside the canonical role definition
     source as the first mandatory case
   Design note:
   - the broader guideline against duplicated raw strings and magic numbers is
     already accepted; this sprint chooses the first low-noise hard-gated
     subset rather than redefining that guideline
   - `"team-lead"` is the first enforced slice because it is already duplicated
     widely enough in the repo to justify a dedicated hard gate now
   - the rule should cover duplicated semantic string literals in non-test Rust
     code, not just this single literal
   - the canonical Rust definition source for the `"team-lead"` literal is
     `crates/atm-core/src/roles.rs` via `ROLE_TEAM_LEAD`
   - exclusions must stay narrow and explicit; the rule must not exempt
     ordinary production code wholesale
   - `TeamName` / `AgentName` newtype hardening remains the longer-term
     structural direction, but this grep-backed gate is still warranted now
     because the postmortem problem is duplicated handwritten literals already
     spread through non-test production code and the full newtype migration is
     larger than this sprint
   Integration:
   - keep this in the existing `identities` lint lane
   Migration posture:
   - ATM-local for now; revisit only if a generic configurable semantic
     literal-policy framework proves worthwhile

3. Family C — fixed-sleep test hygiene
   Development work:
   - add a repository-local lint for fixed `thread::sleep(...)` in ordinary
     Rust test code
   - support an explicit allow-list for the narrow daemon-runtime suite rather
     than silent exceptions
   Integration:
   - wire it into default `just lint`
   Migration posture:
   - ATM-local first; extract only if the rule generalizes cleanly beyond ATM
     test tiers

4. Family D — bare production `Condvar::wait(...)`
   Development work:
   - extend `sc-boundary` to flag bare production `Condvar::wait(...)`
   - treat `wait_timeout(...)` and `wait_timeout_while(...)` as the approved
     production forms
   - require the replacement pattern to inspect the returned
     `WaitTimeoutResult`; merely swapping `wait(...)` for `wait_timeout(...)`
     and discarding the timeout state is not sufficient
   Integration:
   - keep this on the existing `sc-boundary` path so `just lint` runs it
     automatically
   Migration posture:
   - later migrate upstream to standalone `sc-lint` after proving low-noise
     behavior on ATM runtime code

5. Family I — TTL triage-record consistency
   Development work:
   - add a repository-local validator for contradictory
     `triage:status` / `triage:findingAggregate` state in `.ttl` triage records
   Integration:
   - wire it into default `just lint` or the default repo CI lint sequence
     rather than keeping it as a QA-only manual check
   Migration posture:
   - ATM-local unless the team later standardizes the same triage-record schema
     across multiple repos

6. Post-sprint migration review
   Development work:
   - classify which of the implemented ATM-first rules are now sufficiently
     generic to move into standalone `sc-lint`
   Required output:
   - one migration note per family:
     - migrate now
     - keep local
     - extract only framework, not the ATM policy itself

## Sequencing

Recommended build order:
1. Family A in `sc-portability`
2. Family B in the existing identity-literal lane
3. Family C as a new repository-local test-hygiene lint
4. Family D in `sc-boundary`
5. Family I as a repository-local triage validator
6. migration review after all five pass locally

Rationale:
- A and B are the narrowest, least-ambiguous wins
- A lands first because it extends an already-running reusable analyzer lane
- C needs the most allow-list and test-scope tuning
- D benefits from the existing Rust analyzer graph and should land after the
  portability/test-scope patterns are settled
- I is intentionally separate because it validates process artifacts, not Rust
  code

## Acceptance Criteria

- every postmortem family is assigned to one implementation home
- every family is assigned to one integration point:
  - `sc-portability`
  - `sc-boundary`
  - existing repository-local lint lane
  - new repository-local lint lane
- the plan explicitly states which families are expected to migrate to
  standalone `sc-lint` and which are not
- no family remains dependent on repeated QA rediscovery as the primary control

## Required Validation

Planning-only validation for this sprint:
- `just lint`
- any doc-consistency or requirements/architecture review needed to keep the
  sprint plan aligned with the product docs

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/plan-phase-R.md`
- `docs/phase-R/issues.md`

## Risks And Watchouts

- do not over-generalize ATM-specific policy into standalone `sc-lint` before
  the rule semantics are proven on the consumer repo
- keep Family C allow-lists narrow and explicit; otherwise the fixed-sleep rule
  will become noisy and untrusted
- Family D must distinguish bare `wait(...)` from timeout-backed wait forms
- Family I must validate real triage structure rather than depend on fragile
  string ordering

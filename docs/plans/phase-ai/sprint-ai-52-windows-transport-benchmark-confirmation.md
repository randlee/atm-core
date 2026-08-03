---
title: AI.52 Windows transport benchmark confirmation
status: planned
branch: feature/pAI-s52-windows-transport-benchmark
recommended_agent: cwin
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.40
  - AI.49
dependencies_relation:
  - sprint: AI.40
    relation: must_follow
    rationale: M5 must first prove the shared runner and local transport thresholds.
  - sprint: AI.49
    relation: must_follow
    rationale: Windows runs must persist through the durable report contract.
target: integrate/phase-ai-31-33
depends_on: AI.40, AI.49
---

# AI.52 — Windows transport benchmark confirmation

## Recommended Agent / Model

`cwin` / fast: run the same real-daemon admission benchmark on the dedicated
`atm` account on the designated Windows host after AI.40's M5 gate is accepted.

## Execution Dependencies

AI.52 `must_follow`s AI.40 and AI.49. Development may start after both are
pushed; before every development or fix round, merge both into this branch.
Its PR cannot complete until both prerequisite PRs merge into
`integrate/phase-ai-31-33`.

## Goal

Confirm Windows loopback-TCP admission throughput with AI.40's exact public
request and result contract. This is a separate Windows confirmation sprint,
not a substitute for M5 UDS/TCP evidence.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-005B`
- [Cross-platform guidelines](../../cross-platform-guidelines.md)
- ADR-044 — public verification-report classification

## Deliverables

1. On the designated Windows host under the dedicated `atm` OS account, build the branch's
   release CLI/daemon pair and run the standard local smoke ladder: `just
   smoke`, `just smoke localhost`, and `just smoke local-ip`, plus `just test`
   and `atm doctor --json`. Retain its canonical smoke artifacts. Fix a
   local-smoke failure before beginning capacity work. Use an isolated ATM home
   and disposable test database; cwin may reset the designated Windows test
   daemon/database. Never use a shared or production database.
2. Run `just benchmark --transport tcp` through the real release daemon with
   1, 2, 8, 16, and 64 frames per connection. For each profile, retain ten
   independent 1K-message samples using the AI.40 public authenticated
   `POST /v1/atm/messages` path and full response parsing.
3. Persist every result, including failed runs, through AI.49's public-safe
   schema and aggregate benchmark-report path with the safe `windows-x64-01`
   host label. These are the plotted artifacts in Cipher's report, not a
   second Windows report. Record transport, frames, requested/accepted
   messages, elapsed time, request frames/sec, connections/sec, bytes/sec,
   latency, first failure, cleanup, and final doctor health.
4. Treat an error-bearing run as diagnostics, not evidence: root-cause and fix
   request/response, daemon, database, resource, or runner errors, then rerun
   local smoke and the affected profile. For an error-free profile below
   1,000 accepted admissions/responses per second, identify and remove every
   straightforward bottleneck, then rerun. Stop only at an error-free plateau
   whose next improvement requires a deliberate redesign; record that boundary.

## Required Validation

- The complete standard smoke ladder, `just test`, and `atm doctor --json`
  pass before the benchmark; any discovered smoke defect is fixed and
  revalidated before capacity work continues.
- The benchmark uses the branch release daemon and isolated database; no mock,
  direct dispatcher, installed production daemon, or response-body discard.
- Validate the ten JSON artifacts and `just reports-index --check`.
- If code changes, run `just lint`.

## Acceptance Criteria

- M5 evidence and the AI.40 M5 threshold gate are accepted before this sprint
  begins.
- Every Windows profile has ten complete, error-free 1K-message samples. The
  target is 1,000 accepted admissions/responses per second. A clean 700–800/s
  result or a documented one-frame socket/resource ceiling is valid evidence
  only after the full fix/retest loop shows that further improvement needs a
  deliberate redesign; it is not a reason to stop early or a claim that the
  1,000/s target was met.
- Every run has durable report evidence and a passing cleanup/final doctor
  result. Any timeout, response error, partial acceptance, dirty host,
  missing artifact, or threshold miss fails AI.52; diagnostics alone cannot
  close it.

## Non-goals

No Windows OS tuning, UDS benchmark, admission-path optimization, or use of a
production/shared ATM database.

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

## Addendum — exact commands and artifact handling (team-lead, 2026-08-01)

This addendum makes the deliverables directly executable on the Windows host.
It does not change scope; it pins down which repo scripts to run, in what
order, with which flags, and confirms the report-artifact contract already
established by AI.40/AI.49.

### Scripts (already exist in this branch; do not write new ones)

All of the following are `just` recipes wrapping scripts arch-ctm wrote for
AI.33/AI.40 — reuse them as-is:

1. `just smoke` — `scripts/smoke/run_feature_smoke.py normal`
2. `just smoke localhost` — `scripts/smoke/run_feature_smoke.py localhost`
3. `just smoke local-ip` — `scripts/smoke/run_feature_smoke.py local-ip`
4. `just test` — full workspace test suite
5. `atm doctor --json` — daemon/db health check
6. `just benchmark --transport tcp --frames-per-connection <N> ...` —
   `scripts/smoke/run_admission_capacity.py`. Windows only supports
   `--transport tcp` (the script defaults to `uds` on non-Windows, `tcp` on
   `nt`). Run once per frames-per-connection profile: 1, 2, 8, 16, 64. Each
   invocation must build the release CLI/daemon first — `just benchmark`
   already runs `cargo build --release -p agent-team-mail -p atm-daemon`
   before invoking the script, so a separate manual release build is not
   required.
7. `just benchmark-report` — `scripts/smoke/benchmark_report.py`, persists
   the raw run JSON into AI.49's aggregate public-safe schema.
8. `just reports-index --check` — validates the durable report index is not
   stale after new artifacts are added.

### Host label

Set `ATM_CAPACITY_HOST_LABEL=windows-x64-01` (or the actual designated host's
safe label, matching this sprint's `windows-x64-01` naming convention) in the
environment before running `just benchmark`. The runner reads this from
`ATM_CAPACITY_HOST_LABEL` (default `local`) and sanitizes it into the
artifact filename — do not pass it as a script flag.

### Artifact location and git commit

Confirmed against the current `site/reports/send-message-benchmark/` tree
(already populated by M5's Mac-arm64 runs, e.g.
`20260801-165137.900314-m5-arm64-01-tcp-f4.json`): each benchmark run writes
three files per profile — `<timestamp>-<host>-<transport>-f<N>.json`,
`.envelope.json`, and `.xhtml` — directly under
`site/reports/send-message-benchmark/`. This directory is **not**
gitignored (confirmed via `git check-ignore`) and existing runs from other
hosts are already tracked in git. AI.52's ten Windows runs per profile must
land in this same directory using the same naming convention, and **must be
`git add`ed and committed** on this branch alongside any code changes — do
not leave the JSON/xhtml/envelope artifacts untracked. Run
`just reports-index --check` after committing to confirm the index isn't
stale; if it is, run `just reports-index` (no `--check`) to regenerate it and
include that regenerated index file in the same commit.

### Summary output

The aggregate benchmark report (`just benchmark-report`) also writes/updates
under `site/reports/` (the existing `site/reports/send-message-benchmark.html`
aggregate view) — this must be regenerated and committed after all ten
profiles' worth of Windows runs are captured, not just the raw per-run JSON.

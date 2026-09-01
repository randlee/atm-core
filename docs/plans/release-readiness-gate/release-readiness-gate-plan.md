---
title: Release-readiness gate — full-suite evidence run
status: draft
branch: plan/release-readiness-gate
worktree: ../atm-core-worktrees/plan/release-readiness-gate
owner: fenix (coordinator), Rand (authority)
created: 2026-08-31
---

# Release-readiness gate — full-suite evidence run

## 1. Mandate (Rand, 2026-08-31)

- Define a **full release-readiness suite of tests** from the tests that
  already exist. This plan **defines** the suite; it does not create new
  tests. Missing test cases are acceptable and are recorded in §7 as named
  gaps, not silently omitted.
- Add a **new gate** that runs full smoke, full benchmark, and full
  integration tests (atm-hermes-testbed) and produces a complete
  **release evidence set** for the upcoming release.
- The gate must be a **streamlined process that coordinates execution to
  completeness**: one launch drives every suite end-to-end. A launch that
  cannot complete every declared suite fails closed — it never publishes a
  partial evidence set as release evidence.
- **First-time QA pass is a design requirement** (Rand, verbatim intent):
  when this set of tests is launched it runs and everything passes QA the
  first time, assuming there is no performance failure. Multiple rounds of
  QA ceremony because a field was not filled out properly in an evidence
  form are **not acceptable**. Consequence: evidence correctness is enforced
  by the harness at generation time (§2.4), never discovered by a reviewer
  afterward. The only legitimate first-run QA failures are real product
  failures (performance floor breach, test failure) — never clerical.
- **No tests are run now.** This plan, its review, and the gate
  implementation are all definition work. The first execution of the gate
  happens when Rand launches it for the upcoming release.

## 2. Gate contract

### 2.1 Entry point

One command launches the entire gate (working name, final name at review):

```
just release-readiness <release-tag-candidate>
```

- Single launch, no per-suite manual invocation. The runner is the
  coordinator: it sequences suites, retries per-suite bounded recoverable
  setup failures, and does not stop for operator input mid-run.
- Every suite runs in **full** — no sampling, no "quick" profiles, no
  suite skipped silently. A suite that is intentionally not runnable in the
  release environment must be declared in the manifest as `skipped` with a
  recorded reason (e.g. Windows benchmark policy), never absent.

### 2.2 Execution-to-completeness rules

1. The gate runs all suites to a terminal state (pass / fail / declared-skip)
   before rendering any verdict. A crash or abandonment of any suite makes
   the whole gate `INCOMPLETE`, which is a failure state.
2. Evidence is written incrementally per suite, but the **release evidence
   manifest** (§4) is written only after all suites reach a terminal state.
   No manifest ⇒ no release evidence.
3. The gate verdict is `READY` only when every suite passes (or is
   declared-skip with a Rand-approved standing reason) **and** the evidence
   manifest validates against its schema.
4. Fail-closed provenance: every evidence artifact must carry the in-band
   execution-identity fields (execution_account, uid, home, hostname —
   AV-PROD-001R contract). Artifacts missing provenance invalidate the run
   for the suites they belong to.

#### 2.3 First-time-pass enforcement (no clerical QA rounds)

The mechanism that makes §1's first-time-pass requirement real:

1. **One schema, shared by harness and QA.** The evidence/manifest JSON
   schema is committed in-repo and is the *same* artifact the gate validates
   against at generation time and quality-mgr validates against at review
   time. There is no reviewer-side checklist that the harness cannot run
   itself.
2. **Validate-at-emit.** Every suite adapter validates each artifact against
   the schema the moment it is written; an invalid artifact aborts that
   suite as a harness error immediately (fail fast mid-run), not at review.
3. **Whole-set self-verification.** Before rendering a verdict, the gate
   re-validates the complete evidence set + manifest exactly as QA would
   (schema, sha256s, provenance fields, cross-references, reports-index).
   `READY` is unreachable with a clerical defect present.
4. **Rehearsal mode.** `just release-readiness --rehearse` exercises the
   entire evidence pipeline with synthetic suite results — schema, manifest,
   index, self-verification — without running any test. RRG sprints must
   ship a green rehearsal before the gate is declared implemented; the
   rehearsal is also the pre-launch check before the real release run.
   (Rehearsal runs no tests, so it complies with the current no-test-runs
   directive.)
5. **Single QA round by contract.** quality-mgr reviews a real gate run once,
   against the same machine checks plus judgment items (were results
   plausible, floors honest). Clerical findings on a gate run are treated as
   harness defects (fix the validator, not the form) — they can never recur.

### 2.4 Hosts and isolation

| Suite | Host requirement |
|---|---|
| Smoke (full) | isolated benchmark/smoke account (m5-atmbench on rand-m5.local); never an interactive account |
| Benchmarks (send + read/query) | m5-atmbench on rand-m5.local (official-evidence policy); floors gate against committed `baselines.json` |
| Integration (atm-hermes-testbed) | Colima host per testbed topology (as exercised by evidence/colima-v146-smoke, PR #1123) |
| Repo suites (`just ci` etc.) | CI runners (already gate merges); gate records the CI run ids for the release candidate commit rather than re-running locally |

The shared randlee daemon and interactive-account environments are never
touched by the gate (standing constraints).

## 3. Suite inventory (definition — existing tests only)

### 3.1 Smoke — full

- Entry: `just smoke thorough` composition (scripts/smoke/run.py,
  run_feature_smoke.py, run_thorough.py, run_thorough_shared_host.py,
  run_inbound_peer_smoke.py, run_peer_pair.py, daemon_lifecycle.py,
  run_admission_capacity.py).
- Evidence: `site/reports/smoke/<os>/<host>/…` per the smoke-test skill
  contract; committed and indexed.
- Pass criteria: per-lane pass with no suppressed failures; analyze_logs
  clean.

### 3.2 Benchmarks — full

- **Send family**: `just benchmark-official` → evidence under
  `site/reports/send-message-benchmark/`; floors per committed baselines
  (AO2 ratchet convention).
- **Read/query family (Phase AV — lands with PR #1120)**:
  `just benchmark-read` → benchmark-read-fanout, benchmark-query-fts,
  benchmark-read-under-write-load → evidence under
  `site/reports/read-query-benchmark/`; floors per `baselines.json`
  revision 1 (approved 2026-08-31), unrounded p50 comparison, ratchet up.
- Pass criteria: clean-run criteria per each family's D7-style contract;
  all floors met; `just reports-index --check` green.
- Dependency: **PR #1120 must merge before the gate can run the read/query
  family**; the gate definition references it now and the runner refuses to
  render `READY` if a declared family is absent from the tree.

### 3.3 Integration — atm-hermes-testbed (full)

- Fixture: https://github.com/randlee/atm-hermes-testbed (Colima test
  fixture; tier definitions in testbed PR #2, tiers AT0–AT8).
- Entry: testbed runner against the release-candidate build, full tier set
  (prompt tiers + infra tiers), executed in one continuous session.
- Evidence: committed under an `evidence/…` branch/dir in atm-core exactly
  as PR #1123 did, plus sha256 verification of all artifacts.
- Pass criteria (tightened from the PR #1123 lessons — see §7): every tier
  row carries populated `detail`; provenance (digest/SHA/CI run id) recorded
  in-band; version sentinels consistent across prompt files; tier
  definitions in the testbed repo (PR #2 merged) match what is executed 1:1.

### 3.4 Repo test suites (recorded, not re-run)

- `just ci` (lint + test), test-graft-python, test-hermes-graft-bridge,
  test-hermes-graft-smoke, test-admission-capacity,
  test-queue-hooks-python(+codex).
- The gate records the green CI run ids for the exact release-candidate
  commit in the manifest instead of re-executing them on the evidence host.

## 4. Release evidence manifest

One committed JSON manifest per gate run, schema versioned:

- release candidate identity: tag-candidate, commit SHA, branch;
- per-suite entries: suite id, entrypoint, terminal state
  (pass/fail/declared-skip+reason), evidence paths, sha256 of each artifact,
  start/end timestamps, host + execution-identity provenance;
- floors section: each benchmark family's floor vs observed p50;
- CI section: run ids + conclusions for the repo suites (§3.4);
- gate verdict: READY / NOT-READY / INCOMPLETE.

Manifest and all referenced artifacts are committed and pushed
(discarded/unpublished attempts cannot be release evidence — same rule as
benchmark D7). `just reports-index --check` extends to validate the
manifest's referenced paths exist.

## 5. Streamlined coordination (who runs what)

- The gate is launched once by the operator (Rand or a designated runner
  identity) on the evidence host(s).
- The runner script coordinates suite order: smoke → benchmarks →
  testbed integration (order rationale: cheapest-fail-first; final order
  fixed at review). Suites without shared resources may run concurrently
  where the hosts differ (benchmarks on m5-atmbench, testbed on the Colima
  host).
- No mid-run team messaging is required; the run is attended only by its
  runner. Findings from a failed gate route through fenix triage as usual.

## 6. Implementation sprints (post-approval, definition→code)

| # | Deliverable | Assignee (proposed) |
|---|---|---|
| RRG.1 | Manifest schema + `just release-readiness` orchestrator skeleton (suite registry, terminal-state tracking, fail-closed manifest render) | Cipher |
| RRG.2 | Suite adapters: smoke + benchmark families (reuse existing runners; no new harness framework — additive composition only) | Cipher |
| RRG.3 | Testbed integration adapter + testbed-side tier definitions update (merge testbed PR #2; add Phase-AV coverage definitions from §7) | Cipher (atm-hermes-testbed PR) |
| RRG.4 | reports-index manifest validation + docs | Cipher |

All sprints are ordinary dev worktrees off develop with quality-mgr QA;
no test executions beyond each sprint's own new unit tests.

## 7. Known gaps (accepted as missing test cases, recorded not fixed here)

1. **Phase-AV integration coverage in the testbed** (agreed with Rand
   2026-08-31): concurrent read fan-out, read-under-write-load, query/FTS
   functional tests, cross-host read via the Colima topology. To be defined
   in the testbed as part of RRG.3.
2. **Testbed PR #2 is still OPEN**; 28/37 executed rows in the v1.4.6
   evidence had no reviewed definition; AT8 diverged (8 defined → 14
   emitted, renamed). RRG.3 reconciles definitions before the gate's first
   run.
3. **Infra-tier detail fields**: all 27 infra tier rows in the v1.4.6 run
   had `detail:null`; the gate's pass criteria (§3.3) make populated detail
   mandatory.
4. **AT2 skip** filed as atm-core #1121; **AT3 skip** legitimate — both are
   declared-skips until resolved.
5. **Runtime reader gauges** (AV-PROD-002): mixed-mode health signal remains
   the interim 1000 ms p95 ceiling; the manifest records this as a known
   observability limitation until the gauges ship.
6. **Windows**: no isolated Windows benchmark account exists; Windows
   evidence remains a declared-skip per standing policy.
7. **Execution-account admission control** (arch-ctm, post-AV-PROD-001R
   review): the provenance fix *records* the executing account in-band
   (getpwuid(geteuid()), not env-spoofable) but does not *enforce* an
   approved-account allowlist for official runs. If Rand wants official
   evidence rejected — not just flagged — when produced by a non-approved
   account, that is a small scoped harness change; candidate for RRG.2.

## 8. Open questions for Rand (at plan review)

1. Gate launch identity: does Rand launch manually, or is a dedicated
   runner identity/automation wanted for the first release?
2. Should the testbed integration run against the tagged candidate build
   (post-#1120 merge) only, or also pre-merge against integrate builds?

## QA history

*(rounds recorded inline per plan-doc convention)*

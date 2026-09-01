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
  by the harness at generation time (§2.3), never discovered by a reviewer
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
  release environment must be declared in the manifest as `declared-skip`
  with a Rand-approved standing reason (e.g. Windows benchmark policy),
  never absent.

### 2.2 Execution-to-completeness rules

1. The gate runs all suites to a terminal state (`pass` / `fail` /
   `declared-skip`) before rendering any verdict. The suite terminal-state
   taxonomy is exactly the manifest's `terminal_state` enum: `fail` means
   the suite ran and either produced a failing/invalid result or was
   machine-invalidated (rule 4). **INCOMPLETE-by-absence**: a run where any
   suite never reaches a terminal state (crash, abandonment, runner death)
   writes NO manifest at all — the gate outcome `INCOMPLETE` is signaled by
   the absence of a validated manifest for the launch, never by a manifest
   value. `INCOMPLETE` is therefore deliberately unrepresentable in the
   schema (its `verdict` enum is `READY`/`NOT-READY` only): a manifest
   claiming completeness for an incomplete run cannot exist. INCOMPLETE is
   a failure state.
2. Evidence is written incrementally per suite, but the **release evidence
   manifest** (§4) is written only after all suites reach a terminal state.
   The manifest is the **sole release-evidence pointer**: an artifact —
   including per-suite artifacts committed incrementally by existing suite
   conventions (§3.1 smoke) — is release evidence only when referenced,
   with its sha256, by a validated `run_kind: "release"` manifest. Artifacts
   left behind by an incomplete run (no manifest — rule 1) or a failed run
   remain committed history but are inert as release evidence unless a
   validated manifest references them.
3. The gate verdict is `READY` only when every suite passes (or is
   `declared-skip` with a Rand-approved standing reason) **and** the
   evidence manifest validates against its committed schema (§4).
4. Fail-closed provenance: every evidence artifact must carry the in-band
   execution-identity fields (execution_account, uid, home, hostname —
   AV-PROD-001R contract). A suite emitting any artifact without them
   reaches terminal state `fail` with `fail_reason: "provenance-missing"` —
   it is a suite failure, not `INCOMPLETE`, and the run cannot render
   `READY`.
5. Mechanical READY-refusal guards (no forbidden path may depend on operator
   discipline):
   - a declared benchmark family absent from the tree (e.g. read/query
     before PR #1120 merges) ⇒ runner refuses to render `READY`;
   - testbed tier-definition state not pinned and matched — the manifest's
     `testbed_definitions.commit_sha` must identify the tier-definition
     revision actually executed, 1:1 (§3.3) — otherwise the testbed suite is
     `fail` with `fail_reason: "tier-definitions-mismatch"`.

### 2.3 First-time-pass enforcement (no clerical QA rounds)

The mechanism that makes §1's first-time-pass requirement real:

1. **One schema, shared by harness and QA.** The evidence/manifest JSON
   schema is committed in-repo
   ([release-evidence-manifest.schema.json](release-evidence-manifest.schema.json),
   with a worked example in
   [release-evidence-manifest.example.json](release-evidence-manifest.example.json))
   and is the *same* artifact the gate validates against at generation time
   and quality-mgr validates against at review time. The manifest's
   `schema_version` field pins the generation-time schema revision so review
   validates against exactly what the harness enforced. There is no
   reviewer-side checklist that the harness cannot run itself.
2. **Validate-at-emit.** Every suite adapter validates each artifact against
   the schema the moment it is written; an invalid artifact aborts that
   suite as a harness error immediately (fail fast mid-run), not at review.
3. **Whole-set self-verification.** Before rendering a verdict, the gate
   re-validates the complete evidence set + manifest exactly as QA would:
   schema, sha256s, provenance fields, cross-references, reports-index,
   **and the testbed evidence-form classes that caused the v1.4.6 churn
   (§7.2/§7.3): every tier row has populated `detail`, version sentinels are
   consistent across prompt files, and executed tiers match the pinned
   tier-definition revision 1:1**. `READY` is unreachable with a clerical
   defect present.
4. **Rehearsal mode.** `just release-readiness --rehearse` exercises the
   entire evidence pipeline with synthetic suite results — schema, manifest,
   index, self-verification — without running any test. Rehearsal manifests
   carry `run_kind: "rehearsal"` and all rehearsal output is written under a
   dedicated rehearsal root (`site/reports/release-readiness/rehearsal/`)
   that the reports index and release tooling treat as non-evidence; the
   self-verifier owns *both* refusal directions — it refuses a `release`
   manifest that references any path under the rehearsal root, and a
   `rehearsal` manifest that references any path outside it
   (`reports-index --check` re-checks only the release→rehearsal-root
   direction as defense-in-depth; see §4). A green rehearsal is a hard
   acceptance criterion of the final implementation sprint (§6, RRG.4) —
   the gate is not "implemented" without one — and is also the pre-launch
   check before the real release run. (Rehearsal runs no tests, so it
   complies with the current no-test-runs directive.)
5. **Single QA round by contract.** quality-mgr reviews a real gate run
   once. That review consists of (i) re-running the §2.3.3 machine checks —
   which the gate already ran, so they are green by construction — and
   (ii) product-result review **only**: is a reported floor breach / test
   failure real. Floor honesty itself is mechanized: the harness computes
   the manifest `floors` section (observed p50 vs committed baselines,
   ratchet proposals) — QA reviews the product result, not the arithmetic.
   Result plausibility is **not reviewer discretion**: it is the enumerated
   anomaly-check set the harness itself runs as part of §2.3.3 (per-family
   D7 clean-run criteria; observed p50 within the family's historical
   sanity band; wall-clock duration within declared bounds; evidence-row
   counts matching the declared workload). Any anomaly-check hit is emitted
   by the harness as suite `fail` with `fail_reason:
   "measurement-anomaly"`, which is treated as a harness/environment defect:
   fix the check or the environment and re-run — it is never a manual
   override of a computed `READY`, and QA may not fail a `READY` manifest
   on unenumerated plausibility grounds (a missing anomaly check is itself
   a harness defect: add the check, don't hand-adjudicate). There is
   **no third failure category**: a confirmed floor breach / test failure
   renders the run `NOT-READY` as a product failure (Rand's accepted
   exception); any other reviewer finding is by definition a harness
   defect — the fix goes into the validator/self-verifier (never "redo the
   form"), so that class can never recur.

### 2.4 Hosts and isolation

| Suite | Host requirement |
|---|---|
| Smoke (full) | isolated benchmark/smoke account (m5-atmbench on rand-m5.local); never an interactive account |
| Benchmarks (send + read/query) | m5-atmbench on rand-m5.local (official-evidence policy); floors gate against committed `baselines.json` |
| Integration (atm-hermes-testbed) | Colima host per testbed topology (as exercised by evidence/colima-v146-smoke, PR #1123) |
| Repo suites (`just ci` etc.) | CI runners (already gate merges); gate records the CI run ids for the release candidate commit rather than re-running locally |

The shared randlee daemon and interactive-account environments are never
touched by the gate (standing constraints).

### 2.5 Authoritative acceptance checklist (single source)

This checklist is the one place gate acceptance lives; §3 pass criteria and
§6 sprint acceptance criteria reference it and add nothing normative.

- AC-1: one launch (`just release-readiness <tag-candidate>`) drives every
  declared suite to a terminal state without operator input (§2.1, §2.2.1).
- AC-2: manifest written only after all suites terminal; validates against
  the committed schema (closed suite catalog present exactly once, verdict
  `READY`/`NOT-READY` only — an incomplete run writes no manifest,
  INCOMPLETE-by-absence); is the sole release-evidence pointer (§2.2.1,
  §2.2.2, §4).
- AC-3: `READY` requires all suites `pass` or Rand-approved
  `declared-skip`, plus green whole-set self-verification (§2.2.3, §2.3.3);
  anything else renders `NOT-READY`.
- AC-4: every executed suite (`pass`/`fail`) carries `host` + in-band
  execution-identity provenance — schema-required, so
  missing provenance ⇒ suite `fail` (`provenance-missing`); a
  `declared-skip` must NOT carry host/identity; never `READY` with a
  provenance gap (§2.2.4, §4).
- AC-5: mechanical READY-refusal for absent declared benchmark families and
  unpinned/mismatched testbed tier definitions (§2.2.5).
- AC-6: validate-at-emit in every suite adapter (§2.3.2).
- AC-7: self-verification covers schema, sha256s, provenance,
  cross-references, reports-index, tier-row `detail` populated, version
  sentinels consistent, tiers 1:1 vs pinned definitions (§2.3.3).
- AC-8: rehearsal mode green, `run_kind` separation enforced both
  directions, rehearsal output confined to the rehearsal root (§2.3.4).
- AC-9: hosts per §2.4; shared daemon and interactive accounts untouched.
- AC-10: AT2 unresolved (#1121) ⇒ testbed suite `fail`, unless Rand grants
  a standing skip (§7.4, §8 Q3).

## 3. Suite inventory (definition — existing tests only)

### 3.1 Smoke — full

- Entry: `just smoke thorough` composition (scripts/smoke/run.py,
  run_feature_smoke.py, run_thorough.py, run_thorough_shared_host.py,
  run_inbound_peer_smoke.py, run_peer_pair.py, daemon_lifecycle.py,
  run_admission_capacity.py).
- Evidence: `site/reports/smoke/<os>/<host>/…` per the smoke-test skill
  contract; committed and indexed incrementally per the existing skill
  convention. Per §2.2.2 those incremental commits are provisional: they
  become release evidence only when a validated release manifest references
  them.
- Pass criteria: per-lane pass with no suppressed failures; analyze_logs
  clean; acceptance per §2.5.

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
  all floors met; `just reports-index --check` green; acceptance per §2.5.
- Dependency: **PR #1120 must merge before the gate can run the read/query
  family**; per AC-5 the runner refuses to render `READY` if a declared
  family is absent from the tree.

### 3.3 Integration — atm-hermes-testbed (full)

- Fixture: https://github.com/randlee/atm-hermes-testbed (Colima test
  fixture; tier definitions in testbed PR #2, tiers AT0–AT8).
- Entry: testbed runner, full tier set (prompt tiers + infra tiers),
  executed in one continuous session. The build under test is decided by
  Rand at plan review (§8 Q2: tagged candidate build only, or also
  integrate builds); this plan does not preempt that decision.
- Evidence: committed under an `evidence/…` branch/dir in atm-core exactly
  as PR #1123 did, plus sha256 verification of all artifacts.
- Pass criteria (tightened from the PR #1123 lessons — see §7, mechanized
  in §2.3.3/AC-7): every tier row carries populated `detail`; provenance
  (digest/SHA/CI run id) recorded in-band; version sentinels consistent
  across prompt files; tier definitions pinned by testbed commit SHA in the
  manifest (`testbed_definitions`) and matched 1:1 by the executed set.
- AT2/AT3 handling per §7.4 and AC-10.

### 3.4 Repo test suites (recorded, not re-run)

- `just ci` (lint + test), test-graft-python, test-hermes-graft-bridge,
  test-hermes-graft-smoke, test-admission-capacity,
  test-queue-hooks-python(+codex).
- The gate records the green CI run ids for the exact release-candidate
  commit in the manifest instead of re-executing them on the evidence host.

## 4. Release evidence manifest

One committed JSON manifest per gate run. The schema is a committed plan
artifact — [release-evidence-manifest.schema.json](release-evidence-manifest.schema.json)
(JSON Schema 2020-12), with two worked examples:
[release-evidence-manifest.example.json](release-evidence-manifest.example.json)
(a `NOT-READY` run with a floor-breach fail and a declared-skip) and
[release-evidence-manifest.example-ready.json](release-evidence-manifest.example-ready.json)
(a fully green `READY` run — all four suites `pass`, both floors met).
RRG.1 lifts these into their final in-repo home next to the validator; the
schema content in this plan is the reviewed baseline.

Summary of the committed schema (normative source is the schema file):

- `schema_version` (integer) — generation-time schema revision pin;
- `run_kind` — `release` | `rehearsal` (§2.3.4 separation);
- `candidate` — tag-candidate, 40-hex commit SHA, branch;
- `testbed_definitions` — pinned testbed repo + commit SHA (AC-5;
  top-level **required**);
- `suites[]` — **closed catalog**: `suite_id` is the enum
  `smoke-full` / `benchmark-send` / `benchmark-read-query` /
  `testbed-integration`, and four `contains` clauses require every catalog
  suite to be present (duplicate `suite_id` rejection is a §2.3.3
  self-verifier check). Per suite: entrypoint, terminal state
  (`pass`/`fail`/`declared-skip`), evidence paths each with sha256, and
  start/end timestamps. Conditionals: `pass`/`fail` **require** `host` +
  the execution-identity provenance object (AC-4); `fail` **requires**
  `fail_reason` from the closed taxonomy (`provenance-missing`,
  `tier-detail-missing`, `tier-definitions-mismatch`, `sentinel-mismatch`,
  `floor-breach`, `test-failure`, `measurement-anomaly`, `suite-error` —
  a new failure class is a schema revision, never free text);
  `declared-skip` **requires** `skip_reason` and **forbids**
  `host`/`execution_identity` (nothing executed — identity must not be
  fabricated);
- `floors[]` — family (`send` | `read-query`), metric, floor vs observed
  p50, met flag, optional ratchet proposal (harness-computed, §2.3.5);
- `ci_runs[]` — run ids + conclusions for the repo suites (§3.4) pinned to
  the candidate commit;
- `verdict` — `READY` / `NOT-READY` only. `INCOMPLETE` is deliberately
  unrepresentable: an incomplete run writes no manifest at all
  (INCOMPLETE-by-absence, §2.2.1);
- `self_verification` — must record `validated: true` + validator version.

Manifest and all referenced artifacts are committed and pushed
(discarded/unpublished attempts cannot be release evidence — same rule as
benchmark D7).

Rehearsal/release separation ownership (two mechanisms, explicit
direction split): the §2.3.4 **self-verifier** owns *both* directions —
it refuses a `release` manifest referencing any path under the rehearsal
root and a `rehearsal` manifest referencing any path outside it — and is
the acceptance mechanism for AC-8. `just reports-index --check`
additionally validates that the manifest's referenced paths exist and
re-checks the release→rehearsal-root direction as defense-in-depth on
every index run; it is a redundant backstop, not the owner of either
direction.

## 5. Streamlined coordination (who runs what)

- The gate is launched once by an operator on the evidence host(s). Whether
  that operator is Rand personally or a dedicated runner identity is an
  open decision (§8 Q1); nothing in this plan depends on the answer.
- The runner script coordinates suite order. The correctness guarantee is
  **completeness, not order** (§2.2): execution order can never change a
  verdict. Within one host, cheapest-fail-first is the scheduling
  preference (smoke → benchmarks); across hosts, suites without shared
  resources run concurrently (benchmarks on m5-atmbench, testbed on the
  Colima host) — permitted precisely because order carries no correctness
  weight. Final within-host order is fixed at review.
- No mid-run team messaging is required; the run is attended only by its
  runner. Findings from a failed gate route through fenix triage as usual.

## 6. Implementation sprints (post-approval, definition→code)

| # | Deliverable | must_follow | Assignee (proposed) |
|---|---|---|---|
| RRG.1 | Manifest schema (lifted from this plan's committed baseline) + `just release-readiness` orchestrator skeleton (suite registry, terminal-state tracking, fail-closed manifest render) + short ADR recording the terminal-state taxonomy, adapter composition model, and concurrency model (§5) | — | Cipher |
| RRG.2 | Suite adapters: smoke + benchmark families (reuse existing runners; no new harness framework — additive composition only); includes closing §7.8 (send-family execution-identity provenance) so both benchmark families meet AC-4 | RRG.1 | Cipher |
| RRG.3a | atm-core testbed adapter (manifest `testbed_definitions` pinning, AC-5 guard, §2.3.3 tier checks) | RRG.1 | Cipher |
| RRG.3b | Testbed-repo tier definitions: merge testbed PR #2 and add Phase-AV coverage definitions (§7.1). Acceptance: §7.1–§7.3 items resolved (definitions exist for every executed row, AT8 reconciled, detail-population expectations encoded). Fallback if PR #2 stalls (external repo): the gate pins to a Rand-approved testbed commit SHA carrying the reviewed definitions — RRG.3a is not blocked, only the pin value changes | RRG.1. Parallel-safe with RRG.3a: disjoint repos (RRG.3b edits only atm-hermes-testbed; RRG.3a edits only atm-core) — their sole coupling is the pin value RRG.3a's manifest records, fixed at RRG.3b merge (or Rand-approved fallback SHA) | Cipher (atm-hermes-testbed PR) |
| RRG.4 | reports-index manifest validation + rehearsal-root exclusion + docs. Acceptance: **green `--rehearse` run end-to-end** (§2.3.4/AC-8) — the gate is not declared implemented without it | RRG.1–RRG.3a | Cipher |

All sprints are ordinary dev worktrees off develop with quality-mgr QA;
no test executions beyond each sprint's own new unit tests (rehearsal mode
runs no tests). Sprint acceptance = the §2.5 checklist rows each sprint
owns; no sprint-local reinterpretation.

## 7. Known gaps (accepted as missing test cases, recorded not fixed here)

1. **Phase-AV integration coverage in the testbed** (agreed with Rand
   2026-08-31): concurrent read fan-out, read-under-write-load, query/FTS
   functional tests, cross-host read via the Colima topology. To be defined
   in the testbed as part of RRG.3b.
2. **Testbed PR #2 is still OPEN**; 28/37 executed rows in the v1.4.6
   evidence had no reviewed definition; AT8 diverged (8 defined → 14
   emitted, renamed). RRG.3b reconciles definitions before the gate's first
   run.
3. **Infra-tier detail fields**: all 27 infra tier rows in the v1.4.6 run
   had `detail:null`; §2.3.3/AC-7 make populated detail a mechanized check.
4. **AT2 skip**: filed as atm-core #1121 — an open bug, not a standing
   policy. Default disposition: while #1121 is unresolved the testbed suite
   is `fail`, blocking `READY` (AC-10). Rand may instead grant a standing
   declared-skip at plan review (§8 Q3). **AT3 skip** is a legitimate
   standing declared-skip.
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
8. **Send-family execution-identity provenance** (quality-mgr lane-3
   finding, 2026-08-31): the send-benchmark pipeline
   (benchmark_official/benchmark_schema) has no equivalent of the
   AV-PROD-001R in-band execution-identity capture. Closed by RRG.2 so both
   benchmark families satisfy AC-4 before the gate's first run.

## 8. Open questions for Rand (at plan review)

1. Gate launch identity: does Rand launch manually, or is a dedicated
   runner identity/automation wanted for the first release? (§5 is neutral
   to the answer.)
2. Should the testbed integration run against the tagged candidate build
   (post-#1120 merge) only, or also pre-merge against integrate builds?
   (§3.3 defers to this answer.)
3. AT2 (#1121): keep the fail-closed default (testbed suite `fail` while
   unresolved) or grant a standing declared-skip? (§7.4/AC-10.)

## QA history

*(rounds recorded inline per plan-doc convention)*

- **r1 (2026-08-31, quality-mgr verdict FAIL @ 4b5d2dded, msg
  01M1D8BYF5MMQ80D4PE1T66ARZ)**: 4 Blocking, 10 Important, 2 Minor from
  plan-scope + critical-plan reviewers, deduped and re-verified firsthand.
  B1 manifest schema existed only as prose → committed schema + example
  files, `schema_version` pin (§4). B2 undefined third QA category →
  §2.3.5 rewritten: machine checks + product-result judgment only, floor
  arithmetic mechanized, no third category. B3 testbed churn classes not
  mechanized → added to §2.3.3/AC-7. B4 AT2 skip mislabeled → fail-closed
  default + §8 Q3 (AC-10). I1 §1 cited §2.4 for §2.3 machinery + heading
  demotion → fixed. I2 no testbed READY-refusal guard → §2.2.5/AC-5 +
  `testbed_definitions` manifest field. I3 RRG.3 bundling/no fallback →
  split RRG.3a/3b with stall fallback + §7 acceptance mapping. I4 no
  dependency relations → must_follow column. I5 scattered acceptance
  criteria → §2.5 single authoritative checklist. I6 false closure on §8
  open questions → §3.3/§5 made explicitly neutral. I7 rehearsal/release
  indistinguishable → `run_kind` + rehearsal root + two-way refusal. I8
  incremental smoke commits vs manifest-only evidence → §2.2.2
  sole-pointer rule (provisional until referenced). I9 provenance
  invalidation unmapped → suite `fail`/`provenance-missing` (§2.2.4,
  distinct from INCOMPLETE). I10 rehearsal-green unowned → RRG.4 hard
  acceptance criterion. M1 ADR requirement → RRG.1 deliverable. M2
  order-vs-concurrency tension → §5: completeness is the guarantee, order
  carries no correctness weight. Also added §7.8 (send-family provenance
  gap, lane-3 minor (b)) discovered in the same window.
- **r2 (2026-08-31, quality-mgr verdict FAIL @ 65998f2d1, msg
  01M1D9D0FWS1FC59WQBKE4HC55)**: 14/16 r1 findings confirmed fixed; 4
  Blocking, 3 Important, 2 Minor new/residual. B1r2 §2.3.5 "plausibility"
  was reviewer discretion by another name → rewritten: plausibility is the
  enumerated anomaly-check set the harness runs itself (D7 clean-run
  criteria, historical sanity band, duration bounds, evidence-row counts);
  a hit emits suite `fail`/`measurement-anomaly`; QA may not fail a `READY`
  manifest on unenumerated grounds. B2r2 `INCOMPLETE` verdict unreachable
  (manifest only written after all suites terminal) → INCOMPLETE-by-absence:
  dropped from the verdict enum (`READY`/`NOT-READY` only); an incomplete
  run writes no manifest (§2.2.1, §2.2.2, §4). B3r2 `testbed_definitions`
  not in schema `required[]` → now top-level required. B4r2 no suite
  catalog (free-text suite_id, minItems:1) → closed `suite_id` enum + four
  `contains` clauses requiring every catalog suite; duplicates rejected by
  self-verifier. I1r2 free-text `fail_reason` → closed 8-value enum (new
  class = schema revision). I2r2 example fabricated interactive-account
  identity on a declared-skip → schema conditionals: `pass`/`fail` require
  host+identity, `declared-skip` forbids them; example rewritten. I3r2
  example never demonstrated benchmark-send or READY →
  example.json (NOT-READY, all four suites incl. benchmark-send) +
  new example-ready.json (READY). M1r2 §4 vs §2.3.4
  rehearsal-direction ambiguity → explicit ownership: self-verifier owns
  both refusal directions; reports-index --check is release→rehearsal-root
  defense-in-depth only. M2r2 RRG.3b parallel-safe annotation → §6 cell now
  states disjoint repos + sole coupling (pin value). §2.5 AC-2/AC-3/AC-4
  reconciled with the schema changes.

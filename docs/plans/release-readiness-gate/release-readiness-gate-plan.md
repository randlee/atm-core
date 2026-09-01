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
     before PR #1120 merges) ⇒ that family's suite is recorded
     `fail` with `fail_reason: "suite-error"` (the suite could not
     execute at all), so the runner mechanically refuses to render
     `READY`;
   - testbed tier-definition state not pinned and matched — the manifest's
     `testbed_definitions.commit_sha` must identify the tier-definition
     revision actually executed, 1:1 (§3.3) — otherwise the testbed suite is
     `fail` with `fail_reason: "tier-definitions-mismatch"`.
6. **CI-eligibility precondition** (distinct from INCOMPLETE-by-absence):
   before launching any suite, the runner verifies that all seven §3.4
   repo-suite CI runs are green (`success`) on the exact candidate commit —
   a cheap API check, no test execution. If any is red or missing, the gate
   refuses to launch with the named state **"candidate not CI-eligible"**:
   no suite runs, no evidence is produced, and nothing is discarded. A red
   CI run is a genuine product/test failure, but it terminated in CI and is
   recorded there — CI itself is its evidence channel; the gate surfaces it
   pre-launch instead of erasing hours of suite evidence after the fact.
   INCOMPLETE-by-absence stays scoped to rule 1's definition (a suite that
   never reached a terminal state mid-run). The manifest's `ci_runs[]`
   records the verified-green runs; a non-green conclusion is deliberately
   unrepresentable there because the launch never happens.
   Three edge rules seal this precondition:
   - **Emission-time re-verification (no TOCTOU).** GitHub permits
     re-running a completed workflow, so a run's conclusion can flip red
     between the pre-launch check and manifest emission hours later. The
     harness therefore **re-fetches all seven conclusions at
     manifest-emission time**: if any is no longer `success` (or the run
     disappeared), no manifest is written and the refusal record below is
     emitted ("candidate no longer CI-eligible at emission") — a `READY`
     manifest never asserts a conclusion that is red at
     release-decision time. Suite evidence on disk is not discarded, but
     per §2.2.2 it is not release evidence without a validated manifest.
     **Re-launch economics (explicit decision):** an emission-time refusal
     ends the run; there is no resume path. Re-emitting a manifest after
     the candidate is green again means re-running the full gate — the
     accepted cost, because evidence integrity (one continuous gate run
     per manifest, §2.3.5 duration-bounds anomaly check included) outranks
     re-run economics. A resume/evidence-caching mechanism is deliberately
     out of scope; introducing one later is a plan revision, not an
     implementation choice.
     Ownership: RRG.1 owns the pre-launch check; RRG.4 wires the
     emission-time re-check into §2.3.3 self-verification (AC-11).
   - **Refusal is observable.** Every refusal (pre-launch or
     emission-time) writes a **refusal record** conforming to the
     committed schema
     [release-refusal-record.schema.json](release-refusal-record.schema.json)
     (worked example:
     [release-refusal-record.example.json](release-refusal-record.example.json)):
     candidate (tag + 40-hex commit SHA + branch), `refused_at`
     timestamp, `refusal_point` (`pre-launch` | `emission`), and the
     exact red/missing runs (`ci_runs_not_green`, red entries citing the
     run id, missing entries citing none). An emission-point record MAY
     additionally list informational `suite_evidence_paths` already
     produced on disk (never sha256'd, never evidence — the record must
     not grow into a shadow manifest); a pre-launch record must not.
     **Path convention:** records are written under
     `release/refusals/<tag_candidate>/` — outside the evidence roots,
     never under `site/reports/`. An operator can always distinguish
     never-launched from refused-N-times, and see why. This record is
     NOT evidence and is never referenced by a manifest; the writer is
     built once in RRG.1 (explicit acceptance item, §6) and **reused —
     not re-implemented — by RRG.4** for the emission point (explicit
     RRG.4 acceptance item, §6).
   - **Rehearsal carve-out.** For `run_kind: "rehearsal"` (§2.3.4) the
     precondition and the emission-time re-check are **synthesized, not
     fetched**: no GitHub API call is made; the rehearsal harness
     fabricates the seven `ci_runs[]` rows with sentinel run ids
     (1 through 7) and `commit_sha` set to the actual git HEAD commit SHA
     of the tree at rehearsal invocation time, `conclusion`
     `success`. This is safe because rehearsal manifests are structurally
     non-evidence (rehearsal root + two-way refusal, §2.3.4), and it keeps
     RRG.4's green-rehearsal acceptance gate independent of live CI state.
     Accepted residual: beyond `run_kind` and harness discipline, the
     §2.3.4 two-way direction check (rehearsal output confined to the
     rehearsal root, release manifests refusing rehearsal-root paths) is
     the **sole** safeguard distinguishing a sentinel-bearing manifest
     from release evidence — `run_id` values are not independently
     validated against GitHub. This is a recorded acceptance, not an
     oversight.

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
   duplicate-`suite_id` rejection in **both `suites[]` and `ci_runs[]`**
   (the schema's `contains` + `maxItems` clauses already mechanize
   exactly-once; this re-check is defense-in-depth), **the floors
   cross-check** (every §3.2 tracked metric of a pass-terminal benchmark
   suite has a `floors[]` row for its family; no `floors[]` row exists
   for a family whose suite did not execute), **the §2.2.6 emission-time
   CI re-verification** (all seven `ci_runs[]` conclusions re-fetched and
   still `success` — see §2.2 rule 6), **the §2.3.5
   anomaly-check set** (per-family D7 clean-run criteria, historical p50
   sanity band, wall-clock duration bounds, evidence-row counts vs declared
   workload — built in RRG.2, wired into this self-verification in RRG.4),
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
   complies with the current no-test-runs directive.) Rehearsal also does
   not call the GitHub API: the §2.2.6 CI-eligibility precondition and its
   emission-time re-check are synthesized per the §2.2 rule 6 rehearsal
   carve-out (sentinel run ids, rehearsal-HEAD sha), so the RRG.4
   acceptance gate never depends on a real green candidate commit.
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

- AC-1: once the §2.2.6/AC-11 CI-eligibility precondition admits the
  launch, one launch (`just release-readiness <tag-candidate>`) drives
  every declared suite to a terminal state without operator input (§2.1,
  §2.2.1). A refused launch is the one exception: it drives no suites,
  produces no manifest, and writes only the §2.2.6 refusal record.
- AC-2: manifest written only after all suites terminal; validates against
  the committed schema **and** the §2.3.3 self-verifier (the schema
  mechanizes exactly-once catalog completeness for `suites[]` and
  `ci_runs[]` via `contains` + `maxItems`; the self-verifier re-checks
  duplicates as defense-in-depth — AC-7); verdict `READY`/`NOT-READY`
  only — an incomplete run writes no manifest, INCOMPLETE-by-absence; the
  manifest is the sole release-evidence pointer (§2.2.1, §2.2.2, §4).
- AC-3: `READY` requires all suites `pass` or Rand-approved
  `declared-skip`, plus green whole-set self-verification (§2.2.3, §2.3.3);
  anything else renders `NOT-READY`. The `floors[]` requirement for a
  benchmark family is schema-conditional on that family having **produced
  a measurement** (§4): required for `pass` or `fail` with a
  measurement-bearing `fail_reason` (`floor-breach`, `test-failure`,
  `measurement-anomaly`, `provenance-missing`); NOT required for
  `declared-skip` or for `fail`/`suite-error` (the suite could not
  execute, §2.2.5/AC-5) — the schema never forces fabricating a
  measurement that never ran.
- AC-4: every executed suite (`pass`/`fail`) carries `host` + in-band
  execution-identity provenance — schema-required, so
  missing provenance ⇒ suite `fail` (`provenance-missing`); a
  `declared-skip` must NOT carry host/identity; never `READY` with a
  provenance gap (§2.2.4, §4). For a `fail` row whose suite never ran
  (`suite-error`), the provenance identifies the **runner process that
  adjudicated the terminal state** (the orchestrator's own
  host/identity) — always real, never fabricated, so requiring it for
  every `fail` does not recreate the fabricate-or-reject class. The same
  source rule applies to `fail`/`provenance-missing` (rule 4: the
  suite's **own artifact** lacked identity fields): the manifest row's
  `host`/`execution_identity` are populated from the orchestrator's own
  runtime context — recording who adjudicated the provenance gap, never
  guessing what the suite's identity would have been.
- AC-5: mechanical READY-refusal for absent declared benchmark families
  (suite `fail`/`suite-error`) and unpinned/mismatched testbed tier
  definitions (§2.2.5). A `fail`/`suite-error` benchmark row is
  schema-valid **without** a `floors[]` row (AC-3/§4): the mandated
  NOT-READY path never demands a fabricated measurement.
- AC-6: validate-at-emit in every suite adapter (§2.3.2).
- AC-7: self-verification covers schema, sha256s, provenance,
  cross-references, reports-index, duplicate-`suite_id` rejection in both
  `suites[]` and `ci_runs[]` (defense-in-depth behind the schema's
  exactly-once), the floors cross-check (every §3.2 tracked metric of a
  pass-terminal benchmark suite has a `floors[]` row, and no `floors[]`
  row exists for a family whose suite did not execute), the §2.3.5
  anomaly-check set (benchmark/smoke checks built in RRG.2, the
  testbed-scoped duration-bounds check built in RRG.3a — all wired in
  RRG.4), the AC-13 `workspace_version` cross-checks, tier-row
  `detail` populated and conformant to the RRG.3a tier-row schema (§3.3),
  version sentinels consistent, tiers 1:1 vs pinned definitions (§2.3.3),
  and the suite_id↔`fail_reason` compatibility re-check (each suite's
  `fail_reason` drawn from its own schema-scoped subset — defense-in-depth
  behind the schema's per-suite conditionals; a suite-inappropriate
  reason is a mislabel, never a valid row).
- AC-8: rehearsal mode green, `run_kind` separation enforced both
  directions, rehearsal output confined to the rehearsal root (§2.3.4).
- AC-9: hosts per §2.4; shared daemon and interactive accounts untouched.
- AC-10: AT2 unresolved (#1121) ⇒ testbed suite `fail`, unless Rand grants
  a standing skip (§7.4, §8 Q3); tier-level dispositions (per-tier state
  incl. declared-skip, skip-vs-1:1 rule) are encoded per the RRG.3a
  tier-row schema (§3.3).
- AC-11: `ci_runs` closed seven-id catalog, each entry exactly once with
  `conclusion: "success"` pinned to the candidate commit (schema), and the
  §2.2.6 CI-eligibility precondition: all seven runs verified green
  **before any suite launches** (RRG.1) AND **re-verified at
  manifest-emission time** (RRG.4 self-verification) — a red/missing run
  refuses with the point-specific named state ("candidate not CI-eligible"
  pre-launch; "candidate no longer CI-eligible at emission" at emission —
  both distinct from INCOMPLETE-by-absence) and writes the §2.2.6 refusal
  record; never
  `READY`, and never evidence discarded, over a red repo suite — including
  one that went red between launch and emission (§2.2.6, §3.4, §4).
  A refused launch is AC-1's sole carve-out.
- AC-12: **readiness-preflight release gate (§5.1).** The publisher's
  readiness preflight — the rendered
  `.claude/skills/publishing/preflight.xml.j2` assignment executed by the
  named `publisher` teammate per the publishing skill's
  `ref/release-state-strategy.md` (readiness preflight before a `main`
  merge) — must, **mechanically** (RRG.4 delivers the check as code the
  preflight invokes, not checklist prose; §2.3 no-clerical mandate
  applies), before authorizing the `main` merge: (i) locate and
  schema-validate the committed `run_kind: "release"` manifest and
  require `verdict: "READY"`; (ii) verify the diff between the manifest's
  `candidate.commit_sha` and the release-candidate tag commit is
  version-metadata-only (the §5.1 minor-bump commit: `Cargo.toml`
  workspace version, `Cargo.lock`, changelog — nothing else); and
  (iii) verify the release-candidate tag's version equals the manifest's
  `candidate.tag_candidate` — **the label is binding**: evidence approved
  under one proposed version never authorizes releasing another. Any
  check failing ⇒ readiness preflight fails; a non-metadata diff requires
  a fresh gate run (§5.1). The final preflight on the exact `main` commit
  is unchanged and never re-runs the gate.
- AC-13: **per-attempt version distinctness is mechanized (§5.1
  patch++).** The manifest's `candidate` block carries a required
  `workspace_version` (schema-patterned `X.Y.Z`): the orchestrator's
  first act records it from the checked-out gated tree's `Cargo.toml`
  `[workspace.package] version`, and the §2.3.3 self-verifier
  (i) cross-checks the manifest value against that `Cargo.toml` value
  and (ii) refuses emission if any previously committed
  `run_kind: "release"` manifest under the evidence root records the
  same `workspace_version` — a repeated/unbumped version is detected
  mechanically, never asserted in prose. `tag_candidate` is
  schema-patterned to the clean `vX.Y.0` release shape (§5.1; binding
  per AC-12 iii). RRG.1 owns the recording, RRG.4 wires both
  self-verifier checks.

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
  clean — both enforced at generation time by the RRG.2 smoke adapter
  (validate-at-emit, AC-6), same wiring as the other three suites;
  acceptance per §2.5.

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
  executed in one continuous session (validated by a **testbed-scoped
  wall-clock duration-bounds anomaly check** — built in RRG.3a with the
  other tier checks, wired into §2.3.3 whole-set self-verification in
  RRG.4, emitting `fail`/`measurement-anomaly` per the §2.3.5 emission
  convention; the testbed `fail_reason` subset admits it for exactly this
  check). The build under test is decided by
  Rand at plan review (§8 Q2: tagged candidate build only, or also
  integrate builds); this plan does not preempt that decision.
- Evidence: committed under an `evidence/…` branch/dir in atm-core exactly
  as PR #1123 did, plus sha256 verification of all artifacts.
- Pass criteria (tightened from the PR #1123 lessons — see §7, mechanized
  in §2.3.3/AC-7): every tier row carries populated `detail`; provenance
  (digest/SHA/CI run id) recorded in-band; version sentinels consistent
  across prompt files; tier definitions pinned by testbed commit SHA in the
  manifest (`testbed_definitions`) and matched 1:1 by the executed set.
- **Tier-row evidence schema (RRG.3a acceptance deliverable)**: the
  per-tier artifact — the exact evidence-form class that caused the v1.4.6
  churn — gets its own committed schema + worked example, delivered by
  RRG.3a and referenced from AC-7/AC-10: tier id drawn from the pinned
  definition set, non-null `detail`, version-sentinel field, per-tier state
  enum including `declared-skip`, and the explicit rule for how a skipped
  tier interacts with the 1:1 tier-match check. It is deliberately NOT
  committed in this plan: the tier-row shape is co-owned by the external
  testbed repo whose definitions RRG.3b is still reconciling — pinning it
  now would freeze a premature contract. RRG.3a is not acceptable without
  it.
- AT2/AT3 handling per §7.4 and AC-10; the tier-level representation of
  those dispositions is part of the RRG.3a tier-row schema above.

### 3.4 Repo test suites (recorded, not re-run)

- Closed catalog of seven, manifest `ci_runs[].suite_id` ids in
  parentheses: `just ci` (lint + test → `just-ci`), test-graft-python
  (`test-graft-python`), test-hermes-graft-bridge
  (`test-hermes-graft-bridge`), test-hermes-graft-smoke
  (`test-hermes-graft-smoke`), test-admission-capacity
  (`test-admission-capacity`), test-queue-hooks-python
  (`test-queue-hooks-python`), and test-queue-hooks-codex
  (`test-queue-hooks-codex`). The queue-hooks variants are **separate
  entries, each independently green** — there is no combined entry and no
  combination rule to get wrong.
- CI-greenness of all seven runs on the exact release-candidate commit is
  the §2.2.6 **pre-launch eligibility check**: a red or missing run
  refuses the launch ("candidate not CI-eligible") before any suite
  executes — the red run's evidence lives in CI itself. The gate records
  the verified-green run ids in the manifest instead of re-executing them
  on the evidence host; the schema requires all seven entries exactly once
  with `conclusion: "success"`, so a partial `ci_runs` set is
  schema-invalid, never a silently reduced evidence bar.

## 4. Release evidence manifest

One committed JSON manifest per gate run. The schema is a committed plan
artifact — [release-evidence-manifest.schema.json](release-evidence-manifest.schema.json)
(JSON Schema 2020-12), with three worked examples:
[release-evidence-manifest.example.json](release-evidence-manifest.example.json)
(a `NOT-READY` run with a floor-breach fail and a declared-skip),
[release-evidence-manifest.example-ready.json](release-evidence-manifest.example-ready.json)
(a fully green `READY` run — all four suites `pass`, both floors met), and
[release-evidence-manifest.example-skip.json](release-evidence-manifest.example-skip.json)
(a `READY` run with a Rand-approved declared-skip of a benchmark family —
no floors row for the skipped family, demonstrating the conditional
floors requirement).
The §2.2.6 refusal record has its own committed schema and worked
example — [release-refusal-record.schema.json](release-refusal-record.schema.json)
and [release-refusal-record.example.json](release-refusal-record.example.json)
— so RRG.1 and RRG.4 implement one reviewed shape rather than each
interpreting prose.
RRG.1 lifts these into their final in-repo home next to the validator; the
schema content in this plan is the reviewed baseline.

Summary of the committed schema (normative source is the schema file):

- `schema_version` (integer) — generation-time schema revision pin;
- `run_kind` — `release` | `rehearsal` (§2.3.4 separation);
- `candidate` — tag-candidate, 40-hex commit SHA, branch;
- `testbed_definitions` — pinned testbed repo + commit SHA (AC-5;
  top-level **required**);
- `suites[]` — **closed catalog, exactly-once schema-mechanized**:
  `suite_id` is the enum `smoke-full` / `benchmark-send` /
  `benchmark-read-query` / `testbed-integration`; four `contains` clauses
  require every catalog suite present and `maxItems: 4` forbids any
  duplicate (the §2.3.3 self-verifier re-checks duplicate `suite_id`
  rejection in both `suites[]` and `ci_runs[]` as defense-in-depth).
  Per suite: entrypoint, terminal state
  (`pass`/`fail`/`declared-skip`), evidence paths each with sha256, and
  start/end timestamps. Conditionals: `pass`/`fail` **require** `host` +
  the execution-identity provenance object (AC-4); `fail` **requires**
  `fail_reason` from the closed taxonomy (`provenance-missing` — rule 4
  provenance gap; `tier-detail-missing` — unpopulated tier-row detail,
  §3.3/§7.3; `tier-definitions-mismatch` — executed tiers not 1:1 vs the
  pinned definitions, §2.2.5; `sentinel-mismatch` — version-sentinel
  inconsistency caught by the §2.3.3 sentinel-consistency check;
  `floor-breach`, `test-failure`, `measurement-anomaly` — §2.3.5;
  `suite-error` — §2.2.5. A new failure class is a schema revision, never
  free text). **Each `suite_id` admits only its own subset**
  (schema-mechanized per-suite conditionals; §2.3.3/AC-7 re-check):
  smoke never carries benchmark/testbed-only reasons, benchmark rows
  never carry tier/sentinel reasons (so a mislabeled benchmark `fail`
  cannot dodge the AC-3 floors-row requirement), testbed rows never carry
  `floor-breach` (the only benchmark-exclusive reason — floors exist only
  for benchmark families). Testbed rows DO admit `measurement-anomaly`:
  §3.3's one-continuous-session guarantee is validated by a
  **testbed-scoped duration-bounds anomaly check** (built in RRG.3a
  alongside the other §2.3.3 tier checks, wired into whole-set
  self-verification in RRG.4) whose failure emission is
  `fail`/`measurement-anomaly` — the §2.3.5 emission convention, applied
  to the testbed suite. The root-level floors conditionals are
  unaffected: they key on benchmark suite_ids only, so a testbed
  `measurement-anomaly` row never forces a floors row;
  `declared-skip` **requires** `skip_reason` and **forbids**
  `host`/`execution_identity` (nothing executed — identity must not be
  fabricated);
- `floors[]` — family (`send` | `read-query`), metric, floor vs observed
  p50, met flag, optional ratchet proposal (harness-computed, §2.3.5).
  **Convention (schema-enforced): `ratchet_proposal` may be present only
  when `met` is true** — the harness never proposes ratcheting from a
  breached measurement. Family-level completeness is schema-mechanized
  **conditionally on a produced measurement** (root-level `allOf`: a
  family's suite reaching `pass`, or `fail` with a measurement-bearing
  `fail_reason` — `floor-breach`, `test-failure`, `measurement-anomaly`,
  `provenance-missing` — requires at least one floors row for that
  family; a Rand-approved `declared-skip` requires none, and neither
  does `fail`/`suite-error` (the AC-5 absent-family path executed
  nothing) — the schema never forces fabricating a measurement that
  never ran, per §2.1/AC-3/AC-5. The measurement-bearing set is a closed
  whitelist: a future fail_reason defaults to NOT forcing a floors row.
  Documented probe case: `fail`/`suite-error` with no floors row for
  that family **validates**; `fail`/`floor-breach` without one is
  rejected); metric-level completeness (every §3.2 tracked
  metric of a pass-terminal benchmark suite has a floors row) and the
  forbid direction (no floors row for a family that produced no
  measurement) are named §2.3.3 self-verifier cross-checks;
- `ci_runs[]` — **closed repo-suite catalog, exactly-once
  schema-mechanized** (§3.4): `suite_id` is the seven-id enum (`just-ci`,
  `test-graft-python`, `test-hermes-graft-bridge`,
  `test-hermes-graft-smoke`, `test-admission-capacity`,
  `test-queue-hooks-python`, `test-queue-hooks-codex` — the queue-hooks
  variants are separate, independently green entries); seven `contains`
  clauses + `minItems`/`maxItems: 7` require each exactly once, each with
  run id and `conclusion` fixed to `success`, pinned to the candidate
  commit. Greenness is verified pre-launch AND re-verified at
  manifest-emission time (§2.2.6 CI-eligibility precondition + TOCTOU
  re-check) — a red/missing run at either point refuses with a §2.2.6
  refusal record, so a non-green conclusion is unrepresentable in a
  manifest and a `READY` manifest is never stale against CI;
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

### 5.1 Release-pipeline placement (trigger and gate point)

- **Trigger (manual, once per candidate):** the operator (§8 Q1) launches
  `just release-readiness <tag-candidate>` on the evidence host(s)
  against the intended `develop` commit. Nothing triggers the gate
  automatically; the §2.2.6 CI-eligibility precondition is the gate's
  own first act, not a separate manual step.
- **Version-number management (Rand, 2026-08-31): patch++ per gate run;
  the minor bump is gated BEHIND a passing run, never ahead of it.**
  - **Every gate run is preceded by a mandated patch bump** on
    `develop` (workspace `version` patch++ in `Cargo.toml` +
    `Cargo.lock` — mechanical commit). Each attempt therefore executes
    at a unique patch version (e.g. 1.5.1, 1.5.2, … across attempts),
    so every attempt's manifest and evidence set are
    version-distinguishable and no two runs ever share a version —
    mechanized, not asserted: the manifest records the gated commit's
    workspace version and the self-verifier enforces both the
    `Cargo.toml` match and cross-attempt uniqueness (**AC-13**). The
    patch++ commit must itself be CI-green before launch (§2.2.6
    applies to the gated commit as always). **Launch sequencing
    (normative):** the launching operator waits until all seven §3.4
    repo-suite runs have reached a conclusion on the just-pushed
    patch++ commit before invoking the gate — the operator owns this
    wait (manual launch, §5.1); an early invocation is safe but wasted:
    the §2.2.6 pre-launch check refuses fail-closed ("candidate not
    CI-eligible", missing-state rows), spending no evidence.
  - **The release version is a minor bump applied only after `READY`**:
    a passing run at e.g. 1.5.7 is followed by the mechanical minor
    bump commit to 1.6.0 (version metadata + changelog, nothing else) —
    the published version is always a clean `X.Y.0`, never the
    accumulated attempt-patch number, and a failing gate never spends a
    minor version. Order after `READY`: (1) minor bump commit lands on
    `develop`; (2) `release-candidate-vX.Y.0` is cut on that bump
    commit via `release-candidate.yml` (publishing skill release-state
    strategy); (3) publisher preflights proceed.
  - The manifest's `candidate.tag_candidate` names the **proposed**
    release version (e.g. `v1.6.0`); `candidate.commit_sha` pins the
    gated commit at its attempt-patch version — the manifest claims
    what was measured, not what will be published.
  - **Readiness-preflight acceptance is match-modulo-bump**: the
    preflight accepts the `READY` manifest for gated commit C against
    candidate-tag commit C′ **iff `diff C..C′` is version-metadata-only
    (the minor bump)** — mechanically verified; any other delta fails
    the preflight and requires a fresh gate run (which starts with its
    own patch++). The seven §3.4 CI suites still run green on C′ as
    ordinary merge hygiene — the manifest's `ci_runs[]` remain pinned
    to C, the commit whose behavior the evidence actually measured (a
    version-string bump changes no measured behavior; that claim is
    exactly what the bump-only diff check verifies).
- **Gate point (publisher readiness preflight — normative home:
  AC-12):** the gate's output — a committed, validated
  `run_kind: "release"` manifest with `verdict: "READY"` — is a
  **required input of the publisher's readiness preflight**, i.e. it
  blocks the `main` merge, before any tag/publish action. The concrete
  enforcement mechanism, the three mechanized checks (READY manifest
  schema-validation; bump-only diff between gated commit and candidate
  tag commit; **binding** `tag_candidate` ↔ candidate-tag version
  equality — evidence approved under one version label never releases
  another), and the code-not-prose requirement live in **AC-12**; RRG.4
  owns delivering that check as code invoked by the publishing skill's
  readiness preflight (`preflight.xml.j2` / `publisher` teammate,
  `ref/release-state-strategy.md`). Because the minor bump lands only
  after `READY` (above), the manifest's `candidate.commit_sha` never
  equals the tag commit by definition — hence AC-12's match-modulo-bump
  rule. A non-metadata diff ⇒ fresh gate run (starting with its own
  patch++); the final preflight on `main` (candidate-tag ancestry) is
  unchanged and never re-runs the gate.

## 6. Implementation sprints (post-approval, definition→code)

| # | Deliverable | must_follow | Assignee (proposed) |
|---|---|---|---|
| RRG.1 | Manifest schema (lifted from this plan's committed baseline, **including the suite_id-scoped `fail_reason` subsets** — per-suite schema conditionals) + `just release-readiness` orchestrator skeleton (suite registry, terminal-state tracking, fail-closed manifest render, **§2.2.6 CI-eligibility precondition check with the "candidate not CI-eligible" refusal state and the refusal-record writer implementing the committed [release-refusal-record.schema.json](release-refusal-record.schema.json) (path convention `release/refusals/<tag_candidate>/`; built once here, reused by RRG.4) — both explicit acceptance items, AC-11**; **explicit acceptance item (AC-13): the orchestrator's first act records the gated tree's `Cargo.toml` workspace version into `candidate.workspace_version`** (self-verifier cross-checks wired in RRG.4)) + short ADR recording the terminal-state taxonomy, adapter composition model, concurrency model (§5), **and the §2.2.6 CI-eligibility/refusal-state semantics (snapshot-vs-emission verification, refusal observability, INCOMPLETE-vs-refusal boundary)** | — | Cipher |
| RRG.2 | Suite adapters: smoke + benchmark families (reuse existing runners; no new harness framework — additive composition only); includes closing §7.8 (send-family execution-identity provenance) so both benchmark families meet AC-4; **builds the §2.3.5 anomaly-check set** (D7 clean-run criteria, sanity band, duration bounds, evidence-row counts) emitting `fail`/`measurement-anomaly` | RRG.1 | Cipher |
| RRG.3a | atm-core testbed adapter (manifest `testbed_definitions` pinning, AC-5 guard, §2.3.3 tier checks); **owns the tier-row evidence schema + example (§3.3)**, **owns the testbed-scoped wall-clock duration-bounds anomaly check** (validates §3.3's one-continuous-session guarantee, emits `fail`/`measurement-anomaly`; wired into whole-set self-verification by RRG.4), and **owns the AC-10/§7.4 AT2/AT3 disposition logic** (fail-closed default, standing-skip encoding) | RRG.1 | Cipher |
| RRG.3b | Testbed-repo tier definitions: merge testbed PR #2 and add Phase-AV coverage definitions (§7.1). Acceptance: §7.1–§7.3 items resolved (definitions exist for every executed row, AT8 reconciled, detail-population expectations encoded). Fallback if PR #2 stalls (external repo): the gate pins to a Rand-approved testbed commit SHA carrying the reviewed definitions — RRG.3a is not blocked, only the pin value changes | RRG.1; parallel-safe with RRG.3a (see note below) | Cipher (atm-hermes-testbed PR) |
| RRG.4 | reports-index manifest validation + rehearsal-root exclusion + docs; **wires the RRG.2 anomaly-check set, duplicate-`suite_id` rejection across both `suites[]` and `ci_runs[]`, the floors cross-check (both directions), the suite_id↔`fail_reason` compatibility re-check, the RRG.3a testbed duration-bounds check, the AC-13 `workspace_version` cross-checks (Cargo.toml match + cross-attempt uniqueness), and the §2.2.6 emission-time CI re-verification into §2.3.3 whole-set self-verification** (AC-7, AC-11, AC-13). **Explicit acceptance item: an emission-time refusal invokes the RRG.1 refusal-record writer with `refusal_point: "emission"` — reused, never re-implemented — so the AC-11 emission path cannot silently skip the record.** **Explicit acceptance item (AC-12): the publisher readiness preflight invokes an RRG.4-delivered mechanized check — READY-manifest schema validation, bump-only diff (gated commit ↔ candidate tag commit), and binding `tag_candidate` ↔ tag-version equality — as code, not checklist prose; the release gate is not deliverable without it.** Acceptance: **green `--rehearse` run end-to-end** (§2.3.4/AC-8, using the §2.2 rule 6 rehearsal carve-out — no live GH API dependency) — the gate is not declared implemented without it | RRG.1–RRG.3a | Cipher |

RRG.3a/RRG.3b parallel-safety rationale: disjoint repos (RRG.3b edits only
atm-hermes-testbed; RRG.3a edits only atm-core). Their sole coupling is the
pin value RRG.3a's manifest records, fixed at RRG.3b merge (or the
Rand-approved fallback SHA).

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
- **r4 (2026-09-01, addressing quality-mgr r3 FAIL @ 856efe41c, msg
  01M1DAASJ032FB3DE85VJ7T3TW; r2 dispositions scored 9/9 truthful)**:
  B1r3 `ci_runs` had no closed catalog (empty/partial schema-valid; READY
  example carried 1 of 6) → closed 6-id enum + `contains` clauses +
  `conclusion` const `success` + minItems 6; §3.4 ids aligned; both
  examples carry all six. B2r3 (B4r2 residual) AC-2 overclaimed
  exactly-once as schema-enforced → AC-2 reworded to schema
  (at-least-once) + self-verifier (duplicate rejection), duplicate
  rejection added as an explicit §2.3.3/AC-7 item wired in RRG.4. B3r3
  anomaly-check set claimed "part of §2.3.3" without being named or owned
  there → named in §2.3.3 and AC-7 with ownership: checks built in RRG.2,
  wired into self-verification in RRG.4 (§6 cells updated). I1r3 tier-row
  evidence artifact had no schema/example or tier-level AT2/AT3
  representation → RRG.3a acceptance deliverable defined in §3.3
  (tier id enum, non-null detail, sentinel field, per-tier state enum
  incl. declared-skip, skip-vs-1:1 rule), referenced from AC-7/AC-10;
  deliberately not committed in-plan (testbed co-ownership, RRG.3b still
  reconciling). I2r3 no symmetric forbids → schema now forbids
  fail_reason/skip_reason off their own terminal states (pass forbids
  both). I3r3 AC-10 disposition logic unowned → RRG.3a cell names it.
  M1r3 absent-family fail_reason named: `suite-error` (§2.2.5/AC-5).
  M2r3 ratchet_proposal-only-when-met stated in §4 and schema-enforced.
  M3r3 example skip row cross-refs AC-10/§7.4. M4r3 RRG.3b rationale
  moved to a note below the §6 table. All schema changes revalidated:
  both examples VALID, 6 new negative cases (partial ci_runs, non-green
  run, fail_reason-on-pass, skip_reason-on-fail, fail_reason-on-skip,
  ratchet-on-unmet-floor) verified REJECTED.
- **r5 (2026-08-31, addressing quality-mgr r4 FAIL @ 8f354e2d2, msg
  01M1DB4VKZH2BVDCR2VR7EC18T; all 10 r3 findings scored FIXED)**:
  B1r4 `ci_runs` catalog had no §2.5 acceptance criterion → new AC-11
  (closed seven-id catalog, exactly-once, const-success, §2.2.6
  precondition), cross-referenced from §3.4/§4; precondition check owned
  by RRG.1 (§6 cell). B2r4 red-CI runs were an evidentiary blackhole
  (const-success unrecordable ⇒ INCOMPLETE-by-absence overloaded) —
  design choice delegated to fenix, option (b) chosen → §2.2 rule 6
  CI-eligibility precondition: before any suite launches, all seven §3.4
  repo-suite runs are verified green on the exact candidate commit; a
  red/missing run refuses the launch with the distinct named state
  "candidate not CI-eligible" (no suite runs, no evidence produced,
  nothing discarded — the red run's evidence lives in CI itself);
  INCOMPLETE-by-absence stays scoped to §2.2.1's definition. I1r4
  duplicate-`suite_id` scoping ambiguous → exactly-once is now
  schema-mechanized for BOTH arrays (`contains` + `maxItems`: suites 4,
  ci_runs 7); self-verifier duplicate rejection retained as
  defense-in-depth, named for both arrays in §2.3.3/AC-2/AC-7. I2r4
  floors had no completeness guard → schema: `minItems: 2` + `contains`
  send + `contains` read-query (family-level); metric-level completeness
  (every §3.2 tracked metric of a pass-terminal benchmark suite has a
  floors row) added as a named §2.3.3/AC-7 self-verifier cross-check.
  I3r4 queue-hooks python/codex combination rule was a clerical trap →
  resolved MORE mechanically than suggested: split into two catalog ids
  (`test-queue-hooks-python`, `test-queue-hooks-codex`; 7-id catalog),
  each independently green — no combined entry, no combination rule to
  get wrong (§3.4, schema enum). M1r4 §3.3 "one continuous session"
  unverifiable → cross-referenced to the §2.3.5 wall-clock
  duration-bounds anomaly check. All schema changes revalidated: both
  examples VALID (each now carries all seven ci_runs), 5 new negative
  cases (duplicate suites row, duplicate ci_runs row, 6-of-7 ci_runs,
  missing read-query floor, empty floors) verified REJECTED.
- **r6 (2026-08-31, addressing quality-mgr r5 FAIL @ 6b445ef0c, msg
  01M1DBVS9V13KX3AKT1Z2WDYSK; all 6 r4 findings scored FIXED, no
  regressions)**: all r5 findings were second-order edges of the r5
  mechanisms themselves. B1r5 unconditional floors family-completeness
  contradicted Rand-approved benchmark declared-skip (schema-forced
  fabrication or unsatisfiable manifest) → option (b), fully
  schema-mechanized: dropped floors `minItems`/unconditional `contains`;
  root-level `allOf` conditionals require a family's floors row exactly
  when that family's suite reached `pass`/`fail` — a declared-skip
  requires none; forbid direction (no row for a non-executed family) is
  a named §2.3.3/AC-7 self-verifier cross-check; new third example
  (example-skip.json: READY with benchmark-read-query declared-skip, no
  read-query floors row) demonstrates the skip path; AC-3/§4 updated.
  B2r5 CI-eligibility TOCTOU (conclusion could flip red between
  pre-launch check and emission) → §2.2 rule 6 emission-time
  re-verification: all seven conclusions re-fetched at manifest emission;
  red/missing ⇒ no manifest + refusal record ("candidate no longer
  CI-eligible at emission"); ownership split RRG.1 (pre-launch) / RRG.4
  (emission re-check in self-verification); AC-11 + schema description
  updated. I1r5 §2.2.6 vs rehearsal undefined → rehearsal carve-out in
  rule 6 + §2.3.4: precondition and re-check synthesized, no GH API;
  fabrication defined (sentinel run ids 1–7, rehearsal-HEAD sha,
  `success`), safe because rehearsal manifests are structurally
  non-evidence; RRG.4 acceptance criterion references it. I2r5 AC-1
  contradicted the refusal path → AC-1 reworded with the precondition as
  admission condition and the refused launch as sole carve-out; mutual
  AC-1/AC-11 xref. I3r5 refusal unobservable → §2.2 rule 6 non-evidence
  refusal record (outside evidence roots; candidate, timestamp, refusal
  point pre-launch|emission, red/missing run ids), never
  manifest-referenced; explicit RRG.1 acceptance item (§6). I4r5 RRG.1
  ADR scope omitted §2.2.6 → ADR topics now include CI-eligibility/
  refusal-state semantics (snapshot-vs-emission, refusal observability,
  INCOMPLETE-vs-refusal boundary). Schema revalidated: all THREE examples
  VALID; negative cases re-run — executed-family-missing-floors REJECTED
  (send pass without send row; read-query pass without read-query row),
  duplicate rows and 6-of-7 ci_runs still REJECTED; skip-without-floors
  VALID only when the family's suite is declared-skip.
- **r7 (2026-08-31, addressing quality-mgr r6 FAIL @ 4a6fcb17b, msg
  01M1DCMPRX5CDQYR98ABDFDFYX; all 6 r5 findings scored FIXED, no
  regressions; 1B/4I/4M)**: B1r6 `fail`/`suite-error` still forced a
  fabricated floors row (second instance of the fabricate-or-reject
  class, probe-confirmed: AC-5's own mandated NOT-READY scenario was
  schema-unsatisfiable without inventing `observed_p50`) → floors
  conditionals narrowed to a **produced measurement**: the root `allOf`
  `if` now fires on `pass` OR `fail` with a measurement-bearing
  `fail_reason` (`floor-breach`, `test-failure`, `measurement-anomaly`,
  `provenance-missing` — a closed whitelist, so future fail reasons
  default to NOT forcing a row); `declared-skip` and `fail`/`suite-error`
  require none; AC-3/AC-5/§4 updated; documented probe case added
  (suite-error without floors row validates). Fail-as-single-bucket
  sweep run over the remaining schema conditionals per quality-mgr's
  trajectory note: the `pass`/`fail` ⇒ host+execution_identity
  conditional is intentionally kept (AC-4 note added: for `suite-error`
  the provenance is the adjudicating runner's own identity — real, not
  fabricated). I1r6 refusal-state naming contradiction → point-specific
  names applied consistently ("candidate not CI-eligible" pre-launch,
  "candidate no longer CI-eligible at emission") at §2.2 rule 6, AC-11,
  and the schema ci_runs description. I2r6 refusal record had no
  committed shape → new committed
  release-refusal-record.schema.json + release-refusal-record.example.json
  (candidate, refusal_point, refused_at, ci_runs_not_green with
  red-requires-run_id / missing-forbids-run_id conditionals) and the
  path convention `release/refusals/<tag_candidate>/`; §2.2 rule 6, §4,
  and RRG.1 reference the schema instead of prose field lists. I3r6
  emission path could skip the record → explicit RRG.4 acceptance item:
  reuse the RRG.1 writer with `refusal_point: "emission"`, never
  re-implement. I4r6 re-launch economics silent → explicit decision
  recorded in §2.2 rule 6: no resume path, full re-run accepted
  (evidence integrity outranks re-run cost); resume/caching is
  deliberately out of scope and would be a plan revision. M1r6
  "rehearsal HEAD" → reworded to the actual git HEAD commit SHA at
  rehearsal invocation time. M2r6 → §3.1 pass criteria explicitly
  RRG.2-adapter-enforced (validate-at-emit, AC-6). M3r6 → optional
  informational `suite_evidence_paths` on emission-point refusal records
  (schema-limited: never on pre-launch records, never sha256'd — the
  record cannot grow into a shadow manifest). M4r6 → recorded acceptance
  in §2.2 rule 6: the §2.3.4 two-way direction check is the sole
  rehearsal/release safeguard; sentinel run_ids are not independently
  validated. Additionally (Rand question at r7): new §5.1 fixes the
  gate's release-pipeline placement — manual launch after the
  release-candidate tag exists; the validated READY manifest for the
  exact candidate commit is a required input of the publisher's
  readiness preflight (blocks the `main` merge); final preflight on
  `main` unchanged. Schema revalidated: all THREE manifest examples +
  refusal example VALID; new probes — suite-error-without-floors VALID,
  floor-breach-without-floors REJECTED, pre-launch record with
  suite_evidence_paths REJECTED, missing-state record with run_id
  REJECTED; all prior negative cases still REJECTED.
- **r7 amendment (2026-08-31, Rand version-management directive during
  r7 review)**: §5.1 extended — the version bump is gated behind the
  gate: the gate runs at develop's current version, `tag_candidate` is
  the proposed version label only, and the minor bump commit may land
  only after a READY manifest exists (bump → candidate cut → publisher
  preflights). Consequence made explicit (Rand): the manifest's
  `candidate.commit_sha` never equals the candidate tag commit by
  definition; readiness preflight acceptance is match-modulo-bump — the
  tag commit's diff against the gated commit must be
  version-metadata-only (mechanically verified), anything else requires
  a fresh gate run. `ci_runs[]` stays pinned to the gated commit; the
  seven CI suites additionally run green on the bump commit as ordinary
  merge hygiene. A NOT-READY/refused/incomplete run therefore never
  strands a spent version number.
- **r8 (2026-08-31, fix round for quality-mgr r7 FAIL @ 1fe4b07be —
  1B/4I/1M — plus Rand's second version-management directive)**:
  - Rand directive (supersedes part of the r7 amendment): **patch++ per
    gate run** — every gate attempt is preceded by a mandated mechanical
    patch-bump commit on `develop`, so each attempt executes at a unique
    patch version and no two runs share a version; the **minor bump to
    `X.(Y+1).0` lands only after `READY`** (the published version is
    always a clean `X.Y.0`, never an accumulated attempt-patch like
    1.6.7). §5.1 version-management block rewritten accordingly;
    match-modulo-bump preflight rule unchanged in substance.
  - B1r7 (§5.1 requirements had no acceptance home) → new **AC-12** in
    §2.5 (READY-manifest input + bump-only diff + binding tag_candidate
    check, mechanized, named enforcement location) + explicit AC-12
    acceptance item in §6 RRG.4 row; §5.1 gate-point bullet now defers
    normatively to AC-12.
  - I1r7 (global fail_reason enum permits suite-inappropriate mislabels)
    → schema: per-suite `fail_reason` subset conditionals (smoke:
    test-failure/provenance-missing/suite-error; benchmarks: + floor-
    breach/measurement-anomaly, no tier/sentinel reasons; testbed: tier/
    sentinel reasons + test-failure/provenance-missing/suite-error, no
    floor-breach/measurement-anomaly); suite_id↔fail_reason
    compatibility re-check named in §2.3.3/AC-7 and wired in RRG.4.
  - I2r7 (provenance-missing row's identity source unstated) → AC-4
    extended: orchestrator's own runtime context populates the row,
    same source rule as the suite-error carve-out.
  - I3r7 (documentation-only enforcement) → AC-12 names the concrete
    mechanism (`preflight.xml.j2` assignment / `publisher` teammate /
    `ref/release-state-strategy.md` readiness preflight) and requires
    the check be delivered as code the preflight invokes (RRG.4
    acceptance), not checklist prose.
  - I4r7 (tag_candidate binding ambiguity) → resolved **binding**:
    AC-12 check (iii) requires candidate-tag version == manifest
    `tag_candidate`; evidence approved under one version label never
    releases another.
  - M1r7 → closed-taxonomy prose now cross-references every enum value
    (tier-detail-missing → §3.3/§7.3; sentinel-mismatch → §2.3.3
    sentinel-consistency check; etc.).
  - Schema revalidated (probe battery + new suite-scoping probes): all
    prior positives/negatives unchanged; benchmark fail with
    tier-detail-missing / sentinel-mismatch now REJECTED; testbed fail
    with floor-breach REJECTED; smoke fail with measurement-anomaly
    REJECTED; testbed fail with tier-definitions-mismatch still VALID;
    examples all VALID.
- **r9 (2026-08-31, fix round for quality-mgr r8 FAIL @ cee04a817 —
  2B/2I/1M, all introduced by r8's own additions)**:
  - B1r8 (testbed subset forbade the reason §3.3's own guarantee emits)
    → **arm (a)** chosen: `measurement-anomaly` added to the testbed
    subset; a **testbed-scoped wall-clock duration-bounds anomaly
    check** is explicitly owned — built in RRG.3a (with the other tier
    checks), wired into whole-set self-verification in RRG.4 — and §3.3
    now cites that check instead of the (RRG.2-scoped) §2.3.5 set.
    Noted per the fix guidance: the root floors conditionals need no
    carve-out — they key on benchmark suite_ids only, so a testbed
    measurement-anomaly row never forces a floors row (probe-verified).
    Testbed subset now excludes only `floor-breach`.
  - B2r8 (patch++ mandate had no acceptance home, "no two runs share a
    version" unmechanized) → new **AC-13**: required
    `candidate.workspace_version` field (schema-patterned `X.Y.Z`)
    recorded by the orchestrator's first act from the gated tree's
    `Cargo.toml`; self-verifier cross-checks (i) manifest value ==
    Cargo.toml value and (ii) no previously committed release manifest
    records the same value (cross-attempt uniqueness). Explicit RRG.1
    acceptance item (recording) + RRG.4 wiring (both checks); §5.1
    cross-references AC-13.
  - I1r8 (patch++ CI-greenness launch sequencing unstated) → normative
    §5.1 sentence: the launching operator owns waiting for all seven
    §3.4 runs to reach a conclusion on the patch++ commit before
    invoking the gate; an early invocation refuses fail-closed
    pre-launch, spending no evidence.
  - I2r8 (examples taught stale `v1.5.0-rc1`) → all four examples now
    carry `tag_candidate: "v1.6.0"`; the three manifest examples add
    `workspace_version: "1.5.7"` (gated attempt-patch version, teaching
    the §5.1 model: measured at 1.5.7, proposing v1.6.0).
  - M1r8 (tag_candidate free text) → schema-patterned `^v\d+\.\d+\.0$`
    (clean release shape, consistent with binding AC-12 iii).
  - Schema + examples revalidated: full probe battery green; new
    probes — testbed fail/measurement-anomaly VALID (and forces no
    floors row), testbed fail/floor-breach still REJECTED, benchmark
    fail/tier-sentinel reasons still REJECTED, missing
    `workspace_version` REJECTED, malformed `workspace_version`/
    `tag_candidate` (e.g. `v1.6.1`, `1.6.0`) REJECTED.

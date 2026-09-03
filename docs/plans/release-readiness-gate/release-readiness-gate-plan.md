---
title: Release readiness — full-suite evidence run
status: process (rewritten 2026-09-03 per Rand; supersedes the code-gate plan)
branch: plan/release-readiness-gate
worktree: ../atm-core-worktrees/plan/release-readiness-gate
owner: fenix (coordinator), Rand (authority)
created: 2026-08-31
---

# Release readiness — full-suite evidence run

## 1. Mandate

Rand, 2026-08-31: define a full release-readiness suite from the tests that
already exist, run full smoke, full benchmarks and the full
atm-hermes-testbed integration set, and produce a complete evidence set for
the upcoming release. Missing test cases are recorded as gaps (§5), not
silently omitted. Evidence forms must be right the first time; QA rounds
spent on clerical evidence defects are not acceptable.

Rand, 2026-09-03 (supersedes the previous revision of this document): the
readiness run is a **message-coordinated process over the harnesses that
already exist**, not a new harness. The earlier `just release-readiness`
runner, per-suite adapters, evidence-manifest writer, self-verifier and the
RRG.1–RRG.4 code sprints are withdrawn. Nothing in this document is
implemented as code.

## 2. The process (three steps)

Coordinator: fenix. Candidate: one named `develop` commit at a unique patch
version (§4). Every step below runs against that commit and says so in its
report.

1. **Benchmarks on atmbench (fenix, over ssh).** Run the read and write
   benchmark families on the isolated `m5-atmbench` account on
   rand-m5.local and generate the reports with the existing benchmark
   harness:
   - `just benchmark-official` (send family) → `site/reports/send-message-benchmark/`
   - `just benchmark-read` (read/query family, Phase AV) → `site/reports/read-query-benchmark/`
   - `just smoke thorough` in the same session → `site/reports/smoke/<os>/<host>/`
   - `just reports-index --check` green; reports committed and pushed.
   Precondition: `just benchmark-read` and
   `site/reports/read-query-benchmark/baselines.json` land with Phase AV
   (PR #1120); until that merges, step 1 covers the send family only and
   READY is not reachable (§5 item 7).
   Floors: committed `baselines.json` per family (AO2 ratchet convention;
   read/query revision 1, unrounded p50). A missed floor is a product
   failure, never re-run to pass.
2. **Colima integration (request to fenix@atm-dev on rand-m5).** Send an
   ATM task to the M5 team-lead asking them to run the atm-hermes-testbed
   full tier set (AT0–AT8) on their Colima host against the candidate
   commit and commit the reports under an `evidence/…` branch exactly as
   PR #1123 did (sha256 of artifacts, provenance in-band, every tier row
   with populated `detail`, version sentinels consistent).
3. **Windows benchmarks (cwin, in parallel with step 2).** Ask Rand to
   relay the same benchmark request (step 1 commands) to cwin; cwin is
   never dispatched directly (VPN / fastpc4). Windows throughput is
   expected around 80% of Mac by design; the result is recorded, not
   gated on parity.

Steps 2 and 3 run concurrently; step 1 runs whenever atmbench is free.
Repo suites (`just ci`, test-graft-python, test-hermes-graft-bridge,
test-hermes-graft-smoke, test-admission-capacity, test-queue-hooks-python,
test-queue-hooks-python-codex) are not re-run; their green CI runs on the
candidate commit are cited by run id.

## 3. Evidence hand-in and verdict

- Each runner replies by ATM with (sender of record: fenix for step 1,
  fenix@atm-dev on rand-m5 for step 2, Rand for step 3, relaying cwin's
  report paths): candidate commit, host/account, the
  report paths or evidence PR, and pass/fail per family or tier.
- fenix collects the three replies plus the CI run ids into one QA
  dispatch to quality-mgr (reviewers: rust-qa-agent and req-qa over the
  reports; no code review needed). quality-mgr returns one verdict:
  **READY** (every family and tier passed or carries a Rand-approved
  declared-skip) or **NOT-READY** with the failing items.
- A NOT-READY item routes through fenix triage as a normal finding. The
  fix lands on develop, the patch version bumps (§4) and the affected
  step re-runs; steps whose evidence is unaffected by the fix are not
  re-run.
- A missing reply is NOT-READY by absence; there is no partial READY.

## 4. Versioning and release placement

- Before each attempt: mechanical patch++ of the workspace version on
  develop (`Cargo.toml` + `Cargo.lock`), CI green. Each attempt therefore
  has a distinct version in its reports.
- After READY: minor bump to `X.Y.0` on develop, release PR to `main` on
  Rand's approval, `release-candidate-vX.Y.0` cut from the bump commit,
  publisher preflights as today. The publisher preflight cites the READY
  verdict message and the evidence commits; no manifest file is required.
  The `preflight.xml.j2` publishing template therefore takes only the
  publish-channel `manifest_path`; the readiness-manifest variable added
  by round 12 is reverted in this PR.

## 5. Known gaps (recorded, not fixed here)

1. No testbed coverage yet for Phase AV read fan-out, read-under-write,
   query/FTS and cross-host read.
2. Testbed PR #2 (tier definitions) still open; v1.4.6 evidence had 28/37
   rows without a reviewed definition and AT8 diverged. Reconcile before
   the first run.
3. AT2 skip is bug #1121, not policy: testbed tier fails while unresolved
   unless Rand grants a declared-skip. AT3 skip is a standing declared-skip.
4. Runtime reader gauges (AV-PROD-002) not shipped; interim 1000 ms p95
   ceiling stands.
5. No isolated Windows benchmark account; cwin results are informational.
6. Send-family benchmark reports do not capture the executing account
   in-band the way the read family does after AV-PROD-001R.
7. The read/query benchmark family (`just benchmark-read`, its
   `baselines.json`) does not exist on develop until Phase AV PR #1120
   merges; step 1 is incomplete before that.

## 6. Open questions for Rand

1. Does the testbed run against the tagged candidate build only, or also
   against integrate builds?
2. AT2 (#1121): fail-closed or standing declared-skip for this release?

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
- **r10 (2026-08-31, fix round for quality-mgr r9 FAIL @ 093b30c23 —
  1B/3I/2M, all propagation/definition seams of r9's own additions)**:
  - B1r9 (AC-13's cross-attempt uniqueness scan executed against an
    undefined "evidence root" lookup space) → §4 now opens with the
    **committed release-manifest path convention**:
    `site/reports/release-readiness/<tag_candidate>-<workspace_version>/manifest.json`;
    `run_kind: "release"` manifests live ONLY there, the rehearsal root
    (`site/reports/release-readiness/rehearsal/`) is excluded from every
    release lookup, and the refusal dir is outside it entirely. AC-12(i)
    and AC-13(ii) both cite the convention by name; the manifest
    schema's `workspace_version` description cites it too.
  - I1r9 (AC-13 not propagated to summarizing sites) → §2.3.3's
    self-verification enumeration now includes the AC-13
    `workspace_version` cross-checks; §6 RRG.1 row aligned with the
    two-ordered-acts wording + §4 path convention; §6 RRG.2 row scoped
    to the benchmark/smoke instances of the §2.3.5 anomaly-check set
    with a cross-ref to RRG.3a's testbed instance.
  - I2r9 (two contradictory "first act" claims: version recording vs
    §2.2.6 precondition) → §5.1 trigger bullet defines the gate's
    **first two acts, in order**: (1) record `workspace_version`
    (AC-13), (2) run the §2.2.6 CI-eligibility precondition; AC-13 and
    the RRG.1 row restate the same ordering.
  - I3r9 (duration bounds had no committed source of truth) → §2.3.5
    names committed **`release/duration-bounds.json`** (one min/max
    entry per catalog suite, seeded from historical committed evidence
    durations, Rand-approved before first use — baselines.json
    discipline, never self-seeded); RRG.2 declares the smoke +
    benchmark entries, RRG.3a the testbed entry (ownership recorded in
    both §6 rows).
  - M1r9 (refusal-record candidate block lagged the manifest's) →
    refusal schema's `tag_candidate` now patterned `^v\d+\.\d+\.0$`
    (same shape, same candidate) and a `workspace_version`
    (patterned `X.Y.Z`) added. *(Corrected in r11 per I1r10: the field
    is REQUIRED — Act 1 always precedes any refusal point, so the
    original present-when/absent-when framing was impossible under the
    plan's own ordering.)*
  - M2r9 (cross-attempt uniqueness assumed serialized launches) → AC-13
    states the §5 single-runner serialized-launch **precondition**
    explicitly: gate launches are serialized; concurrent launches are
    outside the contract.
  - Schema + examples revalidated: full probe battery green; new
    refusal-schema probes — `v1.6.1` tag REJECTED, `1.6.0` (no `v`)
    REJECTED, record without `workspace_version` still VALID (optional),
    record with malformed `workspace_version` REJECTED; all manifest
    probes unchanged.
- **r11 (2026-09-01, fix round for quality-mgr r10 FAIL @ 1b18cb1b4 —
  1B/4I/3M; r9 dispositions 4 Fixed / 2 Partial)**:
  - B1r10 (duration-bounds artifact named but not contracted — the
    B1r7/B2r8 unowned-normative-artifact class) → committed
    **`release-duration-bounds.schema.json`** + worked example
    (`release-duration-bounds.example.json`): closed catalog, exactly
    one `min_seconds`/`max_seconds` entry per §3 suite, required
    `revision`/`approved_by`/`effective_from` approval marker per the §3.2
    `baselines.json` revision convention (unapproved seed cannot
    validate). New **AC-14** owns shape, launch-time validation with
    the fail-closed named startup failure **"duration bounds unusable"**
    (missing file / schema-invalid / missing entry / `min >= max` —
    harness defect, never silent skip), emission-time re-validation,
    and RRG.2/RRG.3a entry-declaration + RRG.4 wiring ownership (all
    three §6 rows updated).
  - I1r10 (refusal `workspace_version` presence semantics impossible
    under the plan's own ordering; example violated them) → field now
    **REQUIRED** (Act 1 always records the version before Act 2 — the
    only refusal source — can fire); present-when/absent-when framing
    dropped from schema description, §2.2 rule 6, and the r10 M1r9
    changelog line (corrected in place); example carries
    `workspace_version: "1.5.7"`. The genuinely version-less scenario
    is Act-1 failure — §2.2 rule 7's "candidate version unreadable",
    not a refusal (I4r10).
  - I2r10 (AC-12(i) locate step underdefined) → **§4 lookup algorithm**
    stated in §4 and AC-12(i): glob
    `site/reports/release-readiness/<tag_candidate>-*/manifest.json`
    excluding `rehearsal/`, schema-validate matches, require **exactly
    one** `verdict: "READY"` — zero fails ("no READY manifest for
    candidate"), more than one fails closed ("ambiguous READY
    evidence", never an arbitrary pick); RRG.4's AC-12 acceptance item
    names both edges.
  - I3r10 (refusal records under one constant-`tag_candidate` dir could
    silently overwrite, falsifying refused-N-times) → **per-event
    filename convention** in §2.2 rule 6:
    `<refused_at compact UTC>-<refusal_point>.json`, writer refuses to
    overwrite an existing record (fail-closed); named in the RRG.1
    writer acceptance item.
  - I4r10 (Act-1 failure had no named state) → new **§2.2 rule 7:
    named startup failures**, distinct from the refusal taxonomy and
    INCOMPLETE-by-absence: "candidate version unreadable" (Act-1
    `Cargo.toml` read failure) and "duration bounds unusable" (AC-14);
    both are pre-launch aborts — no suites, no manifest, no refusal
    record, self-describing operator-facing state.
  - M1r10 (RRG.1 cell consumability) → rewritten as five enumerated
    deliverables.
  - M2r10 (seeding discipline repeated verbatim) → r12 consolidated the
    policy at AC-14; §2.3.5 and the §6 rows now cross-reference that one
    canonical acceptance site.
  - M3r10 (min-bound rationale unstated) → AC-14 (and the bounds schema
    description): min flags a short-circuited/truncated workload — work
    skipped, not speed penalized; max flags a stalled/contaminated run.
  - Schema + examples revalidated: full probe battery green; new
    probes — duration-bounds example VALID; missing suite entry,
    extra non-catalog entry, missing `approved_by`/`revision`, and
    malformed bound REJECTED; refusal record without
    `workspace_version` now REJECTED (required); all prior manifest and
    refusal probes unchanged.
- **r12 (2026-09-02, fix round for quality-mgr r11 FAIL @ 49a4ff987 —
  M2r10 plus 2I/3M)**:
  - M2r10 / ATM-QA-001 → AC-14 is the single canonical duration-bounds
    policy site; §2.3.5 now contains only the anomaly-check cross-reference.
  - ATM-QA-002 → a same-second, same-refusal-point filename collision is
    an explicit `refusal-record-collision` harness failure: neither record
    nor manifest is written, and the event is never overwritten, dropped,
    or silently suffixed.
  - ARCH-001 → AC-14 assigns launch-time bounds validation and its named
    startup failure to RRG.1, while RRG.4 owns only emission-time
    re-validation, matching the §6 ownership row.
  - RBQA-F011a → renamed the bounds approval timestamp to
    `effective_from`, matching the §3.2 `baselines.json` convention in the
    schema, example, and plan.
  - RBQA-F011b → AC-13 and §5.1 explicitly reuse
    `.just/run_version.py::workspace_version(repo_root)`; no parallel
    `Cargo.toml` reader is authorized.
  - RBQA-F011c → preflight now carries distinct
    `release_readiness_manifest_path` and `manifest_path` destinations;
    the former is the release-evidence manifest and the latter remains the
    publish-channel manifest. The template fixture and evaluation guidance
    cover both names.

- **Rewrite (2026-09-03, Rand)**: rounds r1–r12 hardened a code gate
  (runner, adapters, manifest schema, self-verifier, RRG.1–RRG.4). Rand:
  "I didn't expect us to create another harness. We already have benchmark
  harness and colima harness … you can't easily run colima from m4
  anyhow." Document rewritten to the three-step message-coordinated
  process above; the committed manifest/duration/refusal schemas and
  examples are removed with it. Earlier rounds remain as history only.
- **r1 (2026-09-03, quality-mgr FAIL @ 6ba171c2a, PR #1139 comment
  5519822940)**: Blocking: `preflight.xml.j2` still hard-required
  `release_readiness_manifest_path` (round-12 addition) → template, eval
  and test reverted to their pre-round-12 form; §4 states it. Important:
  `just benchmark-read` / read-query `baselines.json` absent until PR #1120
  → precondition in §2 step 1 and §5 item 7; step-3 sender of record
  unstated → §3 names Rand. Minor: `test-queue-hooks-codex` →
  `test-queue-hooks-python-codex`.

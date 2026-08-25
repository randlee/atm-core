# Sprint AO2.15 — Hot-Path Performance Guardrails

Status: draft · Branch: `feature/ao2-15-hotpath-guardrails` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: arch-ctm (deliverables 1–2, 5) + Cipher-311d
(deliverables 3–4) · recommended_model: deep-reasoning / fast

## Problem (Rand, 2026-08-24/25)

Three-to-four separate times, changes near the admission hot path (or its
benchmark harness) have cost a ~50% measured throughput drop and 1–2 hours
of senior dev time re-earning numbers we already had. The most recent
incident was not code at all — the controlled baseline was measured with an
unmerged harness (`DEFAULT_WORKERS=512`, per-profile TLS context reuse,
commit `21a830dc8` on `fix/ao2-tls-benchmark-session-reuse`) while every
in-tree run used 64 workers, making all comparisons invalid. Historical
"best" numbers (TCP/TLS ~22.1k/22.3k) also trace to candidate branches
whose merge status is under investigation. Guardrails must therefore fence
three things: **the code path, the harness, and the comparison**.

Prior art this sprint builds on (not from scratch): the
`boundaries/*.toml` + literal-scan idiom in
`crates/atm-architecture/tests/boundary_enforcement.rs`; the AO2.10–13
benchmark pipeline (v4 evidence schema, `baselines.json` D1–D3 ratchet,
`historical-record.json`); the standing quality-mgr baseline-approval gate.

## Deliverables

1. **Hot-path manifest + sentinel lint** (in-repo first; sc-lint extraction
   is deliverable 6):
   - `boundaries/atm-core/hot-path-admission.toml`: names the protected
     files/functions — the local admission write path (`commit_write` in
     `storage_and_nudge_router.rs`), the storage writer/batching loop, the
     admission route dispatch — plus per-region banned-construct classes.
   - Sentinel comments `// HOT-PATH: admission` bracket each protected
     region in source.
   - A new literal-scan test in `atm-architecture` enforces: (a) banned
     constructs inside sentinel regions — added `Mutex`/`RwLock`
     acquisitions, `.await` while a guard is live, `std::fs`/blocking
     calls, `spawn_blocking` introductions, `tracing::` event/span macros
     above the manifest-declared level, `dbg!`/`println!`; (b) sentinel
     integrity — a sentinel removed or moved without a same-PR manifest
     change fails; (c) manifest/source agreement — every manifest region
     exists, every sentinel is manifested.
2. **Touch-triggers-evidence CI gate**: a CI job that fails when a PR's
   diff intersects a manifest region unless the PR description carries a
   benchmark-evidence reference (`benchmark-evidence:` line naming a v4
   campaign id or an explicit `benchmark-evidence: waived-by-quality-mgr`
   token that quality-mgr must have posted). Touching the hot path stops
   being free.
3. **Harness contract hash**: `scripts/smoke/benchmark_contract.py`
   computes a stable hash over the harness's performance-relevant contract
   (worker count, frames profiles, interval shape, TLS-context policy,
   timed-window rules). Every v4 evidence JSON gains
   `harness_contract_hash` (schema bump within v4, additive);
   `benchmark_report.py` and the candlestick/compare tooling **refuse to
   compare results across different contract hashes** (hard error naming
   both hashes), and `baselines.json` records the contract hash its floors
   were set under.
4. **Baseline ancestry rule**: a result may be admitted to
   `historical-record.json` or cited by a `baselines.json` revision only if
   its `source_revision` is an ancestor of `origin/develop` or the current
   `integrate/phase-*` head at admission time (`merge-base --is-ancestor`,
   enforced in the migration/report tooling with a test). Numbers achieved
   on stranded candidate branches can inform work but can never become the
   bar.
5. **CI micro-bench tripwire**: a criterion benchmark of the in-process
   admission loop (mock transport, release profile) with an in-repo
   baseline (`benches/admission-baseline.json`) and a ±15% gate, run on
   ubuntu + macOS CI per PR (~1 min). It cannot replace hardware runs; it
   exists to catch the "50% across the board" class instantly. Baseline
   updates follow the D3 pattern: quality-mgr-approved, ratchet-preferred.
6. **sc-lint extraction proposal**: once deliverable 1 is green in-repo,
   file the generalized design (manifest schema + sentinel regions +
   banned-construct classes + diff-trigger) as an sc-lint issue/PR
   skeleton, referencing this sprint as the proving ground. Deliverable is
   the filed proposal, not the sc-lint implementation.
7. **Scheduled hardware run** (smallest viable): a documented
   `m5-atmbench` nightly invocation of `just benchmark` publishing through
   the AO2.10–13 pipeline, with an ATM message to team-lead on any floor
   breach. Ship as a documented runbook + the alert hook in
   `benchmark_report.py`; actual scheduling (cron/launchd) is an ops step
   recorded in the runbook, not repo automation.

## Acceptance criteria

1. Sentinel lint: fixture tests prove each banned-construct class fails
   inside a region and passes outside; sentinel removal without manifest
   change fails; manifest naming a nonexistent region fails.
2. Evidence gate: a synthetic PR diff touching a manifest region without
   the evidence line fails the CI job; with a campaign id or waiver token
   it passes (job unit-tested via workflow dispatch or act-style fixture).
3. Contract hash: two evidence files with different hashes are refused by
   the compare path with an error naming both; `baselines.json` v-next
   records the hash; changing any contract-relevant harness constant
   changes the hash (test).
4. Ancestry: a fixture result with a non-ancestor `source_revision` is
   rejected from historical-record admission with an actionable error; an
   ancestor one is admitted.
5. Micro-bench: deliberate 30% slowdown injected in a fixture branch of the
   admission loop fails the gate; baseline update path requires the
   quality-mgr approval token.
6. All existing suites green on all three CI lanes; no measurable
   micro-bench overhead on unrelated PRs (job runs only when
   crates/atm-http-runtime, atm-storage*, or the manifest change).
7. sc-lint proposal filed with a link recorded in this doc's QA history.

## Required validation

- Live-verify: one real PR exercise on a scratch branch demonstrating the
  evidence gate and sentinel lint end-to-end before quality-mgr dispatch.
- quality-mgr sign-off on the initial hot-path manifest contents (which
  regions are protected is a policy decision, not a dev choice).

## Non-closure / out of scope

- Landing the 22k optimal-path code itself (separate work, sequenced by
  the in-flight provenance/root-cause investigations; this sprint fences
  whatever optimum lands).
- sc-lint implementation (proposal only).
- TLS-below-floor remediation (separately tracked).
- Automated cron provisioning on m5 (runbook + ops step only).

## Dependencies

- must_follow: AO2.14 (PR #1020) merge — deliverable 1's sentinel
  placement must land on the post-pooling shape of
  `storage_and_nudge_router.rs`/client files to avoid churn.
- Sequencing note: deliverables 3+4 are independent of AO2.14 and MAY be
  split into an early mini-PR off `integrate/phase-ao2` if the phase needs
  the comparison guardrails before AO2.14 closes (team-lead's call).
- parallel_safe: none claimed against other open sprints.

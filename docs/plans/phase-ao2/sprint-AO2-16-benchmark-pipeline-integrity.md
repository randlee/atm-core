# Sprint AO2.16 — Benchmark-Pipeline Integrity (Contract Hash, Ancestry, Tripwire)

Status: draft · Branch: `feature/ao2-16-benchmark-pipeline-integrity` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: Cipher-311d (deliverables 1–2) + arch-ctm
(deliverable 3) · recommended_model: fast / deep-reasoning

Second guardrail sprint (see AO2.15 header for motivation). This sprint
fences the **measurement and the comparison** — the classes that caused the
2026-08-24 incidents (64-vs-512 worker harness drift; 22k numbers quoted
from non-ancestor candidate branches; TLS evidence that never measured
TLS). Deliverables 1–2 are **deliberately `parallel_safe` and
immediately dispatchable** (resolving round-1's deferred split: they ARE
the early comparison guardrail); deliverable 3 alone carries must_follow
triggers — see Dependencies, which is authoritative.

## Deliverables

1. **Harness contract hash.**
   - `scripts/smoke/benchmark_contract.py`:

```python
CONTRACT_FIELDS = {            # explicit in/out boundary:
    "default_workers": int,        # IN
    "frames_profiles": list,       # IN (sorted)
    "interval_shape": dict,        # IN (samples per interval, msgs/sample)
    "tls_context_policy": str,     # IN ("per-profile-reuse" | ...)
    "timed_window_rules": str,     # IN (payload-setup-excluded | ...)
}
# EXCLUDED by design: host_label, daemon source_revision, binary hashes,
# OS, Python version — those are provenance (recorded elsewhere in the
# evidence), not harness contract. Comparing across hosts/revisions is
# legitimate; comparing across harness contracts is not.

def contract_hash(contract: dict) -> str:
    """sha256 over canonical serialization: json.dumps(contract,
    sort_keys=True, separators=(",", ":")) with all floats formatted
    repr-stable via str(); returns 'hc1-' + hexdigest[:16]. The 'hc1-'
    prefix versions the canonicalization itself."""
```

   - v4 evidence JSON gains `harness_contract_hash: "hc1-…"` (additive
     field; `BenchmarkRunResult`/`BenchmarkCampaign` validators updated —
     additive within v4, mirroring how `unreachable_at` extended a stable
     schema in phase-AQ planning). Sample fragment:
     `"harness_contract_hash": "hc1-9f3c2a17d0b44e61"`.
   - Writer (runner) and reader (`benchmark_report.py` compare/candlestick
     paths) both call THE SAME `contract_hash()` — no second
     implementation (grep-gated).
   - Compare/candlestick refuse to compare two results with different
     hashes, erroring with both hashes and both artifact ids.
   - **Pre-hash migration rule (round-1 blocking gap)**: evidence
     predating the field is handled by a backfill pass in the migration
     tooling that computes the hash retroactively from each file's
     recorded harness parameters where all CONTRACT_FIELDS are recorded;
     files missing any field are marked
     `harness_contract_hash: "pre-contract"` — a defined rendering state
     (panel badge "pre-contract evidence"), comparable only with other
     `"pre-contract"` entries, never silently dropped and never erroring
     the pipeline. Backfill classification is one-time and one-way:
     `"pre-contract"` is permanent for that artifact (historical params
     are never retro-completed; a corrected measurement is a new run).
   - `baselines.json` v-next records the contract hash its floors were set
     under; the runner warns-and-FAILs a run whose live hash differs from
     the baseline's (a floor is only meaningful under its contract).
2. **Baseline ancestry rule.** Admission of any result into
   `historical-record.json`, or citation by a `baselines.json` revision,
   requires `git merge-base --is-ancestor <source_revision> <ref>` where
   `<ref>` is `origin/develop` or the current `integrate/phase-*` head;
   the admitted entry stores the audit field
   `ancestry_checked_against_sha: "<full sha of the ref head used>"`
   (immutable once recorded — later rebases of the integration branch
   cannot retroactively orphan the audit trail; the recorded sha proves
   what was checked). Rejection error names the revision, the refs tried,
   and the remediation ("merge the branch or re-measure on an ancestor").
   Implemented by reusing/promoting the existing `is_ancestor_revision()`
   helper (`scripts/smoke/run_admission_capacity.py:508`, already used at
   line 1187 for comparison-evidence acceptance) rather than
   reimplementing the ancestry check — same one-shared-helper discipline
   already applied to `diff_scope.py` elsewhere in this plan.
3. **CI micro-bench tripwire** — an early-warning gate honest about shared
   runners:
   - The bench calls the ONE canonical write pipeline:
     `CanonicalWriteHandler::commit_write` on a `StorageAndNudgeRouter`
     built by the normal constructor with a mock transport adapter —
     never a parallel writer. A NEW literal-scan test covering `benches/`
     is added, modeled on the existing idiom at
     `acknowledgement_cannot_restore_a_second_write_pipeline`
     (crates/atm-architecture/tests/boundary_enforcement.rs:132) — no
     pre-existing `benches/`-scoped test is being extended, since none
     exists yet. The bench file is allowed to reference the canonical
     constructor, forbidden to construct a second pipeline.
   - Methodology (round-1 flake finding): criterion with fixed
     `sample_size`, gate computed on the MEDIAN of 5 criterion runs
     in-job, compared against `benches/admission-baseline.json`:

```json
{ "schema_version": 1,
  "bench": "admission_commit_write",
  "median_ns_per_msg": 10450,
  "tolerance_pct": 25,
  "set_under_runner": "github-hosted-ubuntu-22",
  "approved_by": "quality-mgr",
  "harness_contract_hash": "hc1-…" }
```

     Tolerance is ±25% on hosted runners (wide by design: this is a
     50%-class tripwire, not a precision instrument — precision lives in
     the m5 hardware runs). Per-runner-class baselines
     (`set_under_runner`); a run on an unknown runner class is advisory
     (warn, never fail). Flake-response protocol is IN the doc: two
     consecutive failures on unchanged hot-path code = re-baseline
     request to quality-mgr, not a gate fight; baseline updates follow
     the D3 quality-mgr-approval pattern.
   - CI wiring uses the `ci-scope`-style always-register job-level `if:`
     diff pattern (required-check safe; `on.paths:` forbidden) via
     AO2.15's shared `scripts/ci/diff_scope.py` helper (this sprint does
     NOT reimplement the diff idiom), with the path list:
     `crates/atm-http-runtime/`, `crates/atm-storage*/`, `benches/`, and
     the literal manifest path
     `boundaries/atm-core/hot-path-admission.toml` (quoted here verbatim
     from AO2.15's normative sample for the reader; the IMPLEMENTATION
     hardcodes no second copy — since D3 must_follow AO2.15's merge, its
     CI wiring and fixture read the manifest path from AO2.15's landed
     constant/file directly, making drift structurally impossible).

## Acceptance criteria

1. (D1) `contract_hash()` is deterministic across processes/platform
   (test vectors committed); changing any CONTRACT_FIELD changes the
   hash; writer and reader share one implementation (grep gate: exactly
   one definition of `contract_hash`).
2. (D1) Cross-hash comparison refused with both hashes/ids named;
   same-hash comparison proceeds; `"pre-contract"` entries render with
   the badge, compare only among themselves, and never raise (fixture
   covering all three states).
3. (D1) Backfill: fixture evidence with full recorded params gains a real
   hash; missing-param fixture gains `"pre-contract"`; a second backfill
   run is a no-op.
4. (D1) Runner FAILs (with actionable error) when live contract hash ≠
   `baselines.json` contract hash.
5. (D2) Non-ancestor fixture rejected with the named remediation;
   ancestor fixture admitted WITH `ancestry_checked_against_sha`
   populated; mutating that field on an existing entry fails validation.
6. (D3) Deliberate 30% slowdown fixture fails the gate on the
   median-of-5; unknown runner class is advisory-only; the alternate-
   pipeline literal-scan fails a fixture bench constructing its own
   router/listener; baseline update without the quality-mgr approval
   token fails.
7. All suites green on all three CI lanes; the micro-bench job registers
   (no-op) on PRs not touching the listed paths.

## Required validation

- Fixture suites above; test vectors for the hash committed.
- quality-mgr sign-off on: initial `benches/admission-baseline.json`
  values, the `tolerance_pct`, and the backfill results over the real
  evidence tree (values/dates unchanged — same bar as AO2.12's
  migration audit).

## Non-closure / out of scope

- Hot-path lint and evidence gate (AO2.15); runbook/sc-lint (AO2.17).
- Re-deriving the TLS floor after honest-TLS reruns (separate D3-process
  baseline revision, already flagged to quality-mgr).
- Self-hosted benchmark runners (if hosted-runner flake exceeds the
  protocol's tolerance in practice, that becomes its own proposal).

## Dependencies

- parallel_safe: AO2.14, AO2.15, AO2.17 **for deliverables 1–2** (pure
  Python pipeline/tooling: `scripts/smoke/*`, report/compare, baselines —
  zero Rust surface). Deliverables 1–2 are dispatchable immediately
  (this resolves round-1's "early mini-PR MAY" ambiguity: they ARE the
  early comparison guardrail).
- **Deliverable 3 alone**: must_follow AO2.14 (PR-completion trigger) —
  the bench constructs `StorageAndNudgeRouter` and calls `commit_write`,
  a tighter binding to the pooling-scoped constructor/API than AO2.15's
  sentinel comments, so it takes the SAME dependency posture (this
  corrects round-2's contradictory risk assessment); and must_follow
  AO2.15 (PR-completion) for the shared `diff_scope.py` helper it
  consumes. D3's dev may draft against a stub while waiting; its PR
  lands only after both.

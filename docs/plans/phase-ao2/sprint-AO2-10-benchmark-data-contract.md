# Sprint AO2.10 — Benchmark Data Contract and Runner Emission

Status: draft · Branch: `feature/ao2-10-benchmark-data-contract` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Fixes audit findings A.2 (envelope/publication gap), A.3 (campaign identity
not persisted), A.4 (schema drift, v2-writer/v3-model), A.5 (competing
baselines), A.7 (duplicated derivations), B (dead code, unused ledger seam).
Decisions D1–D3, D11 (UTC-only storage), and D12 (machine-identifier half:
`tcp-tls` in all code, schemas, filenames, and JSON) from
[benchmark-reporting-plan-overview.md](./benchmark-reporting-plan-overview.md)
are binding.

## Deliverables

1. **Schema v4** in `scripts/smoke/benchmark_schema.py`: `BenchmarkRunResult`
   (per-target) and `BenchmarkCampaign` (per computer-run, holding the array
   of 3–4 results). `SUMMARY_SCHEMA_VERSION = 4`. Both `extra="forbid"`,
   `frozen=True`.
2. **Runner emits v4 directly.** Starting point is the four-target matrix
   runner (`BENCHMARK_TARGETS`: sqlite/uds/tcp/tcp-tls, one unskippable
   matrix per invocation) from `origin/fix/ao2-7-direct-peer-benchmark-harness`,
   whose merge into `integrate/phase-ao2` is a dispatch precondition for this
   sprint (see overview, Base-branch precondition) — this sprint changes its
   *emission*, it does not build matrix execution. The runner writes
   per-target compact JSON already valid against `BenchmarkRunResult` (no
   post-hoc migration), and at end of the matrix writes one campaign file
   `site/reports/send-message-benchmark/<campaign_id>.campaign.json` valid
   against `BenchmarkCampaign`.
3. **Machine-classified status with one decision owner.** `status` is
   computed, never caller-supplied, by a single function
   `classify_status()` implemented in `scripts/smoke/benchmark_policy.py`
   (which already docstrings itself as the acceptance/baseline policy
   module — `benchmark_schema.py` stays purely structural, its
   `model_validator` only cross-checks that a stored `status` equals the
   `classify_status()` output): `INCOMPLETE` when any lifecycle stage is
   missing (with mandatory `incomplete_reason`); otherwise `PASS` iff
   `messages_durable == messages_admitted == messages_requested` (100%
   durable writes, D2) and `metrics.admissions_per_second.p50 >=
   baseline.value`; otherwise `FAIL`.
   The legacy decision path `evaluate_profile_thresholds()` (and its
   cross-transport comparison concept — `comparison_ratio` /
   `comparison_strict` / `comparison_required`, e.g. tcp needing a ratio of
   another transport's median) is **explicitly retired, not ported**: the
   per-(host, target) floors in `baselines.json` (D1–D3) replace
   cross-transport ratios as the sole acceptance rule, so the v4 contract
   deliberately has no comparison fields. `evaluate_profile_thresholds()`
   and every call site (including the runner's threshold evaluation and
   comparison-reference loading) are deleted in this sprint.
4. **Missing baseline is a hard error.** If `baselines.json` has no entry for
   the run's `(host_label, target)`, result emission fails with an explicit
   operator-facing error naming the missing pair — never a silent None,
   skipped comparison, or defaulted PASS. (The runner's `host_label` default
   of `local` therefore fails fast unless `local` is deliberately seeded.)
5. **Single reviewed baseline file**
   `site/reports/send-message-benchmark/baselines.json`, validated by a
   `BaselineSet` Pydantic model. Seed macOS values per D1
   (35000/18000/17500/17500). Ratchet rule (D3) documented in the file header
   and enforced by a unit test: a new baselines.json revision may only raise
   values. The runner and report read baselines ONLY from this file; the
   static constants at `benchmark_report.py:49-54`, the `--baseline <file>`
   argument path, and the hardcoded Windows comparison defaults in
   `run_admission_capacity.py` (locate by the `mac-arm64-01` literal and the
   `comparison_required` OS branch, per AC #4's grep gate — line numbers
   drift) are removed. The static per-target constants in
   `benchmark_report.py` (locate by the `TARGET_MSG_PER_SECOND` dict; never
   by line number) are removed with them.
6. **One artifact-id/campaign-id derivation helper** in
   `benchmark_schema.py`, used by both runner and report (removes the
   duplicated timestamp munging at `run_admission_capacity.py:1086-1098` and
   `benchmark_report.py:136-139`).
7. **Default path publishes completely.** `just benchmark` (no args) produces,
   for the run: one per-target JSON per OS-required target (4 on
   macOS/Linux, 3 on Windows) + 1 campaign JSON + envelope(s) consumable
   by `.just/generate_report_index.py`, and fails nonzero if any publication
   artifact cannot be written. Envelope creation no longer depends on the
   `--input` flag.
8. **Deletions:** the unused intent/ledger model in
   `scripts/smoke/benchmark_suite.py` (retain only what `TargetResult`
   validation actually needs, or fold it into `benchmark_schema.py` and delete
   the module); dead helpers `parse_utc` and `safe_label` in
   `benchmark_report.py`; `evaluate_profile_thresholds()` in
   `benchmark_policy.py` plus all its call sites and the cross-transport
   comparison machinery (`comparison_ratio`/`comparison_strict`/
   `comparison_required`, comparison-reference loading) per deliverable 3 —
   `classify_status()` is the single surviving decision owner.

## Contract (normative signatures)

```python
class BaselineEntry(BaseModel, frozen=True, extra="forbid"):
    host_label: str          # SAFE_LABEL pattern
    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    p50_floor: float         # msg/s; ratchet: may only increase across revisions
    approved_by: str         # quality-mgr approval reference
    effective_from: datetime # UTC

class BaselineSet(BaseModel, frozen=True, extra="forbid"):
    schema_version: Literal[1]
    revision: int            # monotonically increasing
    entries: tuple[BaselineEntry, ...]

class BenchmarkRunResult(BaseModel, frozen=True, extra="forbid"):
    schema_version: Literal[4]
    campaign_id: str
    host_label: str
    os: Literal["macos", "windows", "linux"]
    target: Literal["sqlite", "uds", "tcp", "tcp-tls"]
    status: Literal["PASS", "FAIL", "INCOMPLETE"]
    incomplete_reason: str | None      # required iff status == INCOMPLETE
    generated_at: datetime             # UTC, ISO-8601
    source_revision: str               # git SHA
    binary_hashes: dict[str, str]
    frames_per_connection: int
    messages_requested: int
    messages_admitted: int
    messages_durable: int
    metrics: BenchmarkMetrics          # existing model, carried forward
    baseline: BaselineRef              # {revision, p50_floor} snapshot used
    durability_after_restart: DurabilityAfterRestart | None
    direct_sqlite_message_write: DirectSQLiteMessageWrite | None

class BenchmarkCampaign(BaseModel, frozen=True, extra="forbid"):
    schema_version: Literal[4]
    campaign_id: str                   # <UTC-stamp>-<host_label>
    host_label: str
    os: Literal["macos", "windows", "linux"]
    phase: str                         # e.g. "ao2"
    started_at: datetime
    completed_at: datetime | None
    source_revision: str
    results: tuple[BenchmarkRunResult, ...]   # 4 on macOS/Linux, 3 on Windows
    status: Literal["PASS", "FAIL", "INCOMPLETE"]  # roll-up, machine-derived
```

All `datetime` fields across these models are UTC with `Z` suffix (D11); a
validator rejects naive or non-UTC offsets.

**sqlite-target variant rule:** for `target == "sqlite"` (direct storage
writer, no connections or wire frames), `frames_per_connection` is `0` and
the network-only fields of `BenchmarkMetrics` (connection rates, frame rates,
wire-byte counts) are `None`; a `model_validator` requires those fields to be
present for network targets and absent for sqlite — implementers never invent
degenerate network values.

Target matrix is OS-derived, not caller-chosen: macOS/Linux = sqlite, uds,
tcp, tcp-tls; Windows = sqlite, tcp, tcp-tls (uds rejected). Roll-up status:
INCOMPLETE if any result INCOMPLETE or a required target missing; else FAIL if
any result FAIL; else PASS. This is enforced in code, not prose: a
`model_validator` on `BenchmarkCampaign` checks `results` covers the
OS-derived required target set and forces the roll-up to INCOMPLETE (with a
synthesized reason naming the missing target) when any required target is
absent — a campaign whose present results all PASS but which never attempted
a required target must not validate as PASS.

## Acceptance criteria

1. `python3 -c` round-trip: every file the runner writes validates against the
   v4 models with `extra="forbid"`; a mutated key or extra field fails.
2. `just benchmark` on this branch produces exactly **4** per-target JSON on
   macOS/Linux and **3** on Windows (verified by an actual matrix run, not a
   single-transport invocation), one `.campaign.json`, envelopes, and exits
   nonzero when the report/index step fails (injected-failure test).
3. Unit tests: status classification truth table (lifecycle-missing →
   INCOMPLETE; durable < requested → FAIL even above baseline; p50 below floor
   → FAIL; both satisfied → PASS; **required target entirely absent from a
   campaign — e.g. tcp-tls never ran on a macOS campaign — → campaign
   roll-up INCOMPLETE**; **missing `BaselineEntry` for (host_label, target) →
   hard emission error, not PASS/FAIL**); Windows matrix rejects uds;
   sqlite-variant validator (network fields required for network targets,
   forbidden for sqlite); ratchet test rejects a baselines.json revision that
   lowers any value.
4. `grep`-gate: no occurrence of `TARGET_MSG_PER_SECOND` in
   `benchmark_report.py`; no `--baseline` argument in
   `run_admission_capacity.py`; no `mac-arm64-01` literal in the runner;
   no `evaluate_profile_thresholds` and no `comparison_ratio` /
   `comparison_strict` / `comparison_required` identifiers anywhere under
   `scripts/smoke/` (single decision owner is `classify_status()` in
   `benchmark_policy.py`);
   D12: no `tcp+tls` (plus-form) string anywhere under `scripts/smoke/`,
   test fixtures, or emitted JSON artifacts — the machine identifier is
   `tcp-tls` only.
5. `.just/tests/` suite passes on macOS and Windows CI lanes.
6. `baselines.json` present with D1 seed values, revision 1, and a
   quality-mgr approval reference recorded in the PR.

## Required validation

- `just test` (workspace) and `.just/tests` python suite, both CI platforms.
- One live macOS `just benchmark` producing a v4 campaign file, committed as
  evidence on the sprint branch. The run must use a seeded `host_label`
  (D1 seeds `rand-m5`; running under any unseeded label, including the
  runner's `local` default, must demonstrate the hard error instead) and the
  evidence note must state which label was used.
- quality-mgr review includes explicit sign-off on `baselines.json` contents.

## Non-closure / out of scope

- Rendering changes (AO2.11) — this sprint may leave HTML output temporarily
  rendering from v4 via the existing templates' compatible fields.
- Historical files remain unmigrated until AO2.12; `load_result()` keeps
  read-only v1–v3 migration acceptance until AO2.12 lands, then it is removed.
- Windows baseline seed values (AO2.12, D1).

## Dependencies

- must_follow: AO2.6 (writer-batching regression) — merged; benchmark numbers
  achieved there must not regress (standing constraint).
- must_follow: merge of `origin/fix/ao2-7-direct-peer-benchmark-harness`
  (four-target matrix runner; already contains PR #1003's provenance fixes)
  into `integrate/phase-ao2` — hard dispatch precondition; this sprint
  modifies that runner's emission. Branch name per the overview's Base-branch
  precondition, which is authoritative.
- parallel_safe: none — AO2.11/12/13 all consume this contract.

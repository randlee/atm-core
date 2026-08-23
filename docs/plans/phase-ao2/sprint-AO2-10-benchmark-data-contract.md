# Sprint AO2.10 — Benchmark Data Contract and Runner Emission

Status: draft · Branch: `feature/ao2-10-benchmark-data-contract` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Fixes audit findings A.2 (envelope/publication gap), A.3 (campaign identity
not persisted), A.4 (schema drift, v2-writer/v3-model), A.5 (competing
baselines), A.7 (duplicated derivations), B (dead code, unused ledger seam).
Decisions D1–D3 from
[benchmark-reporting-plan-overview.md](./benchmark-reporting-plan-overview.md)
are binding.

## Deliverables

1. **Schema v4** in `scripts/smoke/benchmark_schema.py`: `BenchmarkRunResult`
   (per-target) and `BenchmarkCampaign` (per computer-run, holding the array
   of 3–4 results). `SUMMARY_SCHEMA_VERSION = 4`. Both `extra="forbid"`,
   `frozen=True`.
2. **Runner emits v4 directly.** `scripts/smoke/run_admission_capacity.py`
   writes per-target compact JSON already valid against `BenchmarkRunResult`
   (no post-hoc migration), and at end of a full matrix run writes one
   campaign file `site/reports/send-message-benchmark/<campaign_id>.campaign.json`
   valid against `BenchmarkCampaign`.
3. **Machine-classified status.** `status` is computed, never caller-supplied:
   `INCOMPLETE` when any lifecycle stage is missing (with mandatory
   `incomplete_reason`); otherwise `PASS` iff `messages_durable ==
   messages_admitted == messages_requested` (100% durable writes, D2) and
   `metrics.admissions_per_second.p50 >= baseline.value`; otherwise `FAIL`.
4. **Single reviewed baseline file**
   `site/reports/send-message-benchmark/baselines.json`, validated by a
   `BaselineSet` Pydantic model. Seed macOS values per D1
   (35000/18000/17500/17500). Ratchet rule (D3) documented in the file header
   and enforced by a unit test: a new baselines.json revision may only raise
   values. The runner and report read baselines ONLY from this file; the
   static constants at `benchmark_report.py:49-54`, the `--baseline <file>`
   argument path, and the hardcoded Windows comparison defaults
   (`run_admission_capacity.py:1554-1557,1584`) are removed.
5. **One artifact-id/campaign-id derivation helper** in
   `benchmark_schema.py`, used by both runner and report (removes the
   duplicated timestamp munging at `run_admission_capacity.py:1086-1098` and
   `benchmark_report.py:136-139`).
6. **Default path publishes completely.** `just benchmark` (no args) produces,
   for the run: 4 per-target JSON + 1 campaign JSON + envelope(s) consumable
   by `.just/generate_report_index.py`, and fails nonzero if any publication
   artifact cannot be written. Envelope creation no longer depends on the
   `--input` flag.
7. **Deletions:** the unused intent/ledger model in
   `scripts/smoke/benchmark_suite.py` (retain only what `TargetResult`
   validation actually needs, or fold it into `benchmark_schema.py` and delete
   the module); dead helpers `parse_utc` and `safe_label` in
   `benchmark_report.py`.

## Contract (normative signatures)

```python
class BaselineEntry(BaseModel, frozen=True, extra="forbid"):
    host_label: str          # SAFE_LABEL pattern
    target: Literal["sqlite", "uds", "tcp", "tcp+tls"]
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
    target: Literal["sqlite", "uds", "tcp", "tcp+tls"]
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

Target matrix is OS-derived, not caller-chosen: macOS/Linux = sqlite, uds,
tcp, tcp+tls; Windows = sqlite, tcp, tcp+tls (uds rejected). Roll-up status:
INCOMPLETE if any result INCOMPLETE or a required target missing; else FAIL if
any result FAIL; else PASS.

## Acceptance criteria

1. `python3 -c` round-trip: every file the runner writes validates against the
   v4 models with `extra="forbid"`; a mutated key or extra field fails.
2. `just benchmark` on this branch produces exactly: N per-target JSON, one
   `.campaign.json`, envelopes, and exits nonzero when the report/index step
   fails (verified by an injected-failure test).
3. Unit tests: status classification truth table (lifecycle-missing →
   INCOMPLETE; durable < requested → FAIL even above baseline; p50 below floor
   → FAIL; both satisfied → PASS); Windows matrix rejects uds; ratchet test
   rejects a baselines.json revision that lowers any value.
4. `grep`-gate: no occurrence of `DEFAULT_TARGETS *=` static baseline
   constants in `benchmark_report.py`; no `--baseline` argument in
   `run_admission_capacity.py`; no `mac-arm64-01` literal in the runner.
5. `.just/tests/` suite passes on macOS and Windows CI lanes.
6. `baselines.json` present with D1 seed values, revision 1, and a
   quality-mgr approval reference recorded in the PR.

## Required validation

- `just test` (workspace) and `.just/tests` python suite, both CI platforms.
- One live macOS `just benchmark` producing a v4 campaign file, committed as
  evidence on the sprint branch.
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
- parallel_safe: none — AO2.11/12/13 all consume this contract.

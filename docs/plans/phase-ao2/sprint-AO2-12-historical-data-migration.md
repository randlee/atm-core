# Sprint AO2.12 — Historical Data Migration and Single Historical Record

Status: draft · Branch: `feature/ao2-12-benchmark-history-migration` off
`integrate/phase-ao2` (after AO2.10+AO2.11 merge-forward) · PR target:
`integrate/phase-ao2`
recommended_agent: Cipher-311d · recommended_model: fast

Implements decisions D1 (Windows seeds), D7, D8 of
[benchmark-reporting-plan-overview.md](./benchmark-reporting-plan-overview.md).
Fixes audit findings A.4 (four coexisting shapes), A.5 (baseline scattered
across branches — folds `historical-imports.json` from the unmerged
`fix/ao2-7-report-provenance` branch), A.8 (orphaned files invisible to the
index). We are not rewriting history: every measured value and timestamp is
preserved bit-exactly; only structure is normalized.

## Deliverables

1. **Migration tool** `scripts/smoke/migrate_benchmark_history.py` (one-shot,
   idempotent): reads every legacy JSON under
   `site/reports/send-message-benchmark/` (v1–v3, all four observed shapes),
   groups them into campaigns using the current inference rules
   (host/transport/revision + timestamp adjacency), and emits v4
   `BenchmarkRunResult`/`BenchmarkCampaign` records with `generated_at`,
   metric values, counts, and revisions copied unchanged.
2. **`historical-record.json`** — the single historical record (D7), validated
   by a `HistoricalRecord` Pydantic model: all migrated campaigns, each entry
   carrying `final_best: bool` (the final/best run of its campaign group, the
   only point AO2.11 charts), and a per-(host, target) running-best baseline
   trace implementing the retroactive ratchet (D8): baseline-as-of-run =
   best passing p50 seen so far; each historical result's displayed status is
   re-derived against that value plus the 100%-durable rule where the legacy
   data records counts (where counts are absent, status carries
   `evidence_gap: "durability-counts-missing"` rather than a guessed PASS).
3. **Windows baseline seeds** (D1): computed from the best historical passing
   Windows runs, written into `baselines.json` as a new ratchet-compliant
   revision alongside the macOS D1 seeds.
4. **Verification diff report** emitted by the tool
   (`artifacts/benchmark/migration-audit.json` + rendered summary): for every
   legacy file → migrated record, the exact value/timestamp mapping, so
   quality-mgr can verify accuracy (D7 makes quality-mgr responsible for the
   record's values/dates).
5. **Legacy cleanup on the branch:** migrated per-run legacy JSON files are
   replaced by their v4 equivalents; `historical-imports.json` content is
   folded in and the file removed; `load_result()` v1–v3 migration acceptance
   in `benchmark_report.py` is deleted (per AO2.10 non-closure note); orphaned
   files either join a campaign or are recorded in `historical-record.json`
   under `unattributed` with reason.
6. **Full regeneration:** run `benchmark_report.py --rebuild` so all panels,
   `phase-*.html`, `index.html`, and the report index reflect the migrated
   data — candlesticks now show historical final/best points (closing
   AO2.11's non-closure).

## HistoricalRecord contract (normative — owned here, consumed by AO2.11)

```python
class RatchetPoint(BaseModel, frozen=True, extra="forbid"):
    host_label: str
    target: Literal["sqlite", "uds", "tcp", "tcp+tls"]
    effective_from: datetime           # UTC; start of this baseline value
    p50_floor: float                   # best passing p50 seen so far (non-decreasing)
    source_campaign_id: str            # campaign that set this value

class HistoricalResultEntry(BaseModel, frozen=True, extra="forbid"):
    result: BenchmarkRunResult         # v4, values/timestamps bit-exact from source
    displayed_status: Literal["PASS", "FAIL", "INCOMPLETE"]  # re-derived per D8
    evidence_gap: Literal["durability-counts-missing"] | None
    source_files: tuple[str, ...]      # legacy filenames this entry was migrated from

class HistoricalCampaignEntry(BaseModel, frozen=True, extra="forbid"):
    campaign: BenchmarkCampaign        # v4
    final_best: bool                   # exactly one True per campaign group
    results: tuple[HistoricalResultEntry, ...]

class UnattributedEntry(BaseModel, frozen=True, extra="forbid"):
    source_file: str
    reason: str                        # why it joined no campaign group

class HistoricalRecord(BaseModel, frozen=True, extra="forbid"):
    schema_version: Literal[1]
    generated_from_commit: str         # SHA the migration ran against
    campaigns: tuple[HistoricalCampaignEntry, ...]   # started_at ascending
    ratchet: tuple[RatchetPoint, ...]  # per (host,target), effective_from ascending
    unattributed: tuple[UnattributedEntry, ...]
```

Notes on the contract:

- `HistoricalResultEntry` **wraps `BenchmarkRunResult` unmodified** — the
  result's closed `status` Literal from AO2.10 is untouched;
  `displayed_status` (the D8 ratchet re-derivation) and `evidence_gap` live
  on the wrapper, never inside the v4 result. No amendment to the AO2.10
  contract is required.
- **Implementation ownership:** these model classes are *implemented* in
  `scripts/smoke/benchmark_schema.py` during **AO2.11** (which needs them to
  type and test `candlestick_series()`), exactly as specified here; AO2.11
  ships them alongside an empty-record fixture. AO2.12 consumes the classes
  unchanged. Any divergence AO2.12 discovers is a plan amendment to this
  section plus an AO2.11-compatible code change — never a silent fork.

AO2.11's `candlestick_series()` consumes this model exactly as defined here;
AO2.11 fixture data must validate against it.

## Migration tool contract

```
python3 scripts/smoke/migrate_benchmark_history.py \
    --reports-dir site/reports/send-message-benchmark \
    [--check]        # validate + emit audit diff, write nothing
```

Idempotency: a second run over migrated data is a no-op with exit 0.
Any legacy file it cannot classify is a hard error naming the file — no
silent skips (memory: dont-dismiss-gap-warnings).

## Acceptance criteria

1. Value preservation: automated test asserts, for every legacy fixture and
   for the real tree, that each migrated record's p50/p95/p99/min/max,
   counts, `generated_at`, and `source_revision` are equal to the source
   (string-exact for timestamps, numeric-exact for values).
2. This sprint uses the `HistoricalRecord` classes shipped by AO2.11
   without modification (grep gate: no class redefinition outside
   `benchmark_schema.py`); `historical-record.json` validates against
   `HistoricalRecord`; exactly one
   `final_best` per historical campaign group; ratchet trace is
   non-decreasing per (host, target).
3. `--check` mode passes on the migrated tree; a deliberately corrupted value
   in a fixture makes it fail naming the file.
4. Post-migration `--rebuild` is deterministic and the report index lists the
   newest benchmark entries (audit A.2/A.8 orphan counts drop to zero:
   verified by a test that every campaign in the record or phase dir is
   reachable from `index.html`).
5. `baselines.json` new revision adds Windows entries and lowers nothing
   (ratchet test from AO2.10 passes).
6. `.just/tests` green on macOS and Windows CI lanes.

## Required validation

- Fixture suite covering all four observed legacy shapes (23/25/26/35-key)
  plus one v1 file.
- quality-mgr explicitly reviews `migration-audit.json` and signs off on
  values/dates and on the Windows baseline seeds (D1, D7) — this sign-off is
  a merge gate for the sprint PR.

## Non-closure / out of scope

- Deleting INCOMPLETE-run artifacts (future cleanup, D4).
- Any change to templates or chart code beyond consuming
  `historical-record.json` (AO2.11 owns rendering).
- Raw traces under `artifacts/benchmark/` (gitignored, untouched).

## Dependencies

- must_follow: AO2.10 (v4 models), AO2.11 (`HistoricalRecord` consumer and
  regeneration path). Merge-forward trigger: parent dev push.
- parallel_safe: AO2.13 (disjoint files: migration tool + data vs skill doc +
  Justfile recipes; no shared public contract beyond names fixed in AO2.11).

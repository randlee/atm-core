---
phase: AV
sprint: AV.4
title: Massively parallel read and query benchmarks
branch: feature/av4-read-query-benchmarks
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-av
integration_branch: integrate/phase-av
stack_parent: feature/av3-read-concurrency-gates (dependency is on AV.1b below it) — planned; stack provisioned by task AV.0 (phase plan §4)
status: complete
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1b
    relation: must_follow
    rationale: Benchmarks drive the cutover read path and consume the AV.1a
      D5 metrics seams through it; stacked above AV.1b; restack before every
      round.
  - related: AV.2
    relation: parallel_safe
    rationale: AV.2 edits normative docs only; no intersection with
      benchmark harness/report files.
  - related: AV.3
    relation: parallel_safe
    rationale: Benchmark harness/report files do not intersect architecture
      tests or lint tooling.
---

# AV.4 — Massively parallel read and query benchmarks

## QA history

- **QA-r1 fix — `ef9341eae` (2026-08-31):** closed AV4-B1, AV4-B2,
  AV4-I1, AV4-I2, AV4-M1, AV4-M2, and AV4-M3. The fix reuses the shared
  benchmark schema/report/policy primitives, records the complete D5
  diagnostic skeleton without fabricated values, composes all three named
  Just lanes, gates official runs on AV.1b, records budget and corpus
  provenance, and rejects incomplete artifacts. No live campaign or floor
  seeding was performed because AV.1b remains a must-follow dependency.

Prove — and keep proving — that mailbox reads and queries execute in a
massively parallel manner: a read-serialization regression must become a
failing benchmark campaign, not an anecdote discovered by a timed-out
`atm read`.

## Deliverables

This is the authoritative deliverable checklist. Every listed
deliverable is expected to land at a production-ready level for the
scope this sprint claims; partial or shape-only completion fails the
sprint.

- [ ] D1 — Read benchmark family beside send-message-benchmark:
      concurrent `read`/`peek`/`list` against a seeded multi-team,
      multi-mailbox corpus at high reader counts (fan-out ≥ 32
      concurrent readers), reporting p50 throughput and tail latency
      (p95/p99).
- [ ] D2 — Query benchmark family: search/filtered-list (FTS path)
      under the same parallel load, same metrics. This measures the
      AV.1a D2a search lane (the `SearchReader` re-hosted on the pool
      type with its own `search_pool_size`), not the pre-AV
      single-thread reader; the campaign records the search-lane pool
      size and queue depth exactly as it does for the mailbox lane.
- [ ] D3 — Mixed mode: read/query benchmarks while sustained writer
      activity runs (the defect scenario), asserting read latency stays
      within budget while writes proceed.
- [ ] D4 — Ratcheted floors: per-host-label entries in `baselines.json`
      for the new families, standard 3-clean-run seeding rules; floors
      compare like AO2 (p50 vs. floor, unrounded comparison).
- [ ] D5 — Reader-lane diagnostics in reports: AV.1a D5 metrics
      (queue depth, wait vs. execution time, deadline expiries,
      saturation events) captured per campaign so a floor breach is
      diagnosable from the committed artifact.
- [ ] D6 — Harness/report/schema extensions land via the shared-contract
      rules: separate PR, team-lead visibility, macOS/Windows impact
      stated — **within the benchmark-infra freeze exception below**.

      **Benchmark-infra freeze exception (normative scope bound):** a
      standing directive freezes new benchmark-infrastructure work
      (behavioral rules over machinery; infra:product ratio watched).
      AV.4 exists because phase mandate 8 (phase plan §2, Rand
      2026-08-30/31) explicitly requires read/query benchmark targets
      with ratcheted floors; that mandate is the authorization for this
      sprint and is recorded here as the exception. To keep the
      exception narrow the sprint is bound to **additive family
      registration only**: reuse the existing send-message-benchmark
      runner, report renderer, `baselines.json` ratchet code, manifest
      and `reports-index` contract; add the three families, their
      config, and the AV.1a D5 diagnostics fields. No new harness
      framework, runner abstraction, report renderer, schema version
      bump, or CI job type. Anything beyond additive family registration
      is out of scope and requires Rand's sign-off before dispatch.
      quality-mgr verifies the bound at sprint QA (A6).
- [ ] D7 — Normative workload/baseline contract (reproducibility). The
      following are contract, not guidance; a campaign missing or
      partially implementing any element is a **hard campaign failure**
      (invalid artifact, never seeds or gates a floor):
      - *Entry point (normative names):* the official campaign
        entrypoint is exactly `just benchmark-read`; it runs the three
        exact subordinate targets `just benchmark-read-fanout`,
        `just benchmark-query-fts`, and
        `just benchmark-read-under-write-load`. These names are
        contract (renaming requires a plan amendment). Official
        evidence is produced only by `just benchmark-read`; a
        subordinate target alone is a diagnostic run, never a floor
        seed or gate.
      - *Corpus:* a fixed seeded corpus generated deterministically from
        a recorded seed — message count, size distribution, team/mailbox
        distribution (multi-team: ≥8 teams × ≥4 agents, skew profile
        recorded) committed in the harness config; the corpus generator
        version and seed appear in every report.
      - *Concurrency settings:* fan-out (≥32 concurrent client
        requests), mailbox-lane and search-lane pool size and queue
        depth (production defaults per AV.1a D2/D2a: 4/16 and 2/8 —
        never raised to make a floor) are explicit config recorded per
        campaign; changing any of them starts a new baseline family
        entry, never a comparison against the old floor.
      - *Writer load (mixed mode):* sustained writer rate and payload
        profile fixed in config and recorded in the report.
      - *Mixed-mode read budget:* the interim p95 ceiling is **1000 ms**,
        sourced from the AV.1a D5 contract while runtime reader timing
        metrics are unimplemented. The harness records this named source in
        every mixed-mode report; AV.1b replaces the entry only through a
        reviewed configuration change when exported metrics become available.
      - *Timing windows (every in-scope campaign, reference Mac host):*
        an explicit warm-up window (excluded from metrics) and a
        measurement window, both durations fixed in config and recorded
        in the report; measurement begins only after warm-up completes.
        A future Windows campaign contract is out of scope here and, if
        added later, defines its own windows separately.
      - *Success-rate and latency limits:* a campaign is clean only if
        request success rate meets the configured threshold (≥99.9%
        unless the config records a different value with rationale) and
        p50/p95/p99 are all captured; floors gate on p50, p95/p99 are
        recorded for diagnosis.
      - *Clean-run criteria:* no harness errors, no partial artifacts,
        success-rate threshold met, run completed both windows,
        `just reports-index --check` passes.
      - *Baseline aggregation + ratchet:* the seeded floor is the
        minimum p50 across the 3 seeding campaigns; subsequent clean
        runs ratchet the floor upward per the existing AO2 ratchet
        convention (floor rises to observed p50 minus the recorded
        tolerance percentage; tolerance value committed in
        `baselines.json`, unrounded comparison). Floors never move down
        without an explicitly approved reseed.
      - *Provenance:* the committed baseline entry records the 3 source
        campaign IDs (report paths under `site/reports/`), host label,
        harness version, corpus seed; every campaign report is committed
        and pushed — discarded/unpublished attempts cannot seed floors. Every
        emitted campaign/report JSON also carries an in-band
        `execution_identity` object with `execution_account`, `uid`, `home`,
        and runtime `hostname`; official runs fail closed when any identity
        field cannot be captured. `host_label` remains the operator-selected
        machine label and is not an execution-account assertion. Historical
        artifacts without this additive object remain valid and immutable.

## Contract samples

Indicative `baselines.json` shape for the new families (D4); exact keys
follow the existing send-message-benchmark entry conventions:

```json
{
  "read-fanout": {
    "rand-m4.local": {
      "p50_floor_msgs_per_sec": 0,
      "seeded_runs": 3,
      "ratchet_tolerance_pct": 5,
      "source_campaigns": [
        "site/reports/read/<campaign-1>",
        "site/reports/read/<campaign-2>",
        "site/reports/read/<campaign-3>"
      ],
      "corpus_seed": "<recorded-seed>",
      "fanout": 32,
      "mailbox_pool_size": 4,
      "mailbox_queue_depth": 16,
      "search_pool_size": 2,
      "search_queue_depth": 8,
      "harness_version": "<version>"
    }
  },
  "query-fts": { "rand-m4.local": { "...": "same shape" } },
  "read-under-write-load": { "rand-m4.local": { "...": "same shape, plus writer_rate" } }
}
```

Floor values are seeded from 3 clean runs (never hand-set); `0`/placeholder
values above illustrate shape only. The D7 contract governs every field's
semantics; the exact key names are finalized in the harness PR without
weakening the recorded-provenance requirement.

## Acceptance criteria

This is the authoritative acceptance checklist.

- [ ] A1 — `just benchmark-read` (the sole official entrypoint) runs
      all three families end-to-end on a dedicated benchmark account
      and publishes through the existing manifest/report contract;
      partial artifacts are invalid.
- [ ] A2 — Mixed-mode campaign demonstrates reads meeting budget while
      the writer sustains load; the report shows both read latency and
      concurrent write throughput.
- [ ] A3 — Floors are seeded from 3 clean runs (per D7 clean-run
      criteria) on the reference host and committed to `baselines.json`
      with full D7 provenance (3 source campaign IDs, corpus seed,
      settings, harness version); a synthetic serialization regression
      (e.g. pool size forced to 1 in a scratch build) fails the campaign
      (demonstrated once, then reverted).
- [ ] A5 — D7 contract enforced by the harness: a run with a missing or
      partially implemented contract element (no warm-up window, corpus
      seed absent, success-rate below threshold, partial artifact)
      terminates as a hard campaign failure and cannot be published as
      evidence.
- [ ] A4 — Official evidence only from isolated non-interactive
      accounts (m5-atmbench); no interactive-account campaign is cited.
- [ ] A6 — Freeze-exception bound (D6) verified: the sprint diff touches
      only family registration/config, report field additions, and
      baseline entries; no new runner, renderer, schema version, or CI
      job type. Mixed-mode reports (D3) include the AV.1a D5 WAL-health
      gauges so checkpoint starvation under read-plus-write load is
      visible in the committed artifact.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`, `just test` (harness unit tests)
- [ ] One full `just benchmark-read` campaign on the reference Mac host
      (m5-atmbench account) with committed artifacts under
      `site/reports/`
- [ ] `just reports-index --check`
- [ ] A3 scratch-regression demonstration recorded in sprint QA history.

## Out of scope

- Changing send-message-benchmark floors or harness behavior.
- Windows read-benchmark campaigns (follows the existing deferred
  Windows evidence policy; revisit after the send-path Windows work).
- Fixing the suspected batch-and-wait pipelining issue in the send
  harness (separately tracked, post-AO2).

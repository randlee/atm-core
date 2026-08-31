---
phase: AV
sprint: AV.4
title: Massively parallel read and query benchmarks
branch: feature/av4-read-query-benchmarks
integration_branch: integrate/phase-av
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1b
    relation: must_follow
    rationale: Benchmarks drive the cutover read path and consume the AV.1a
      D5 metrics seams through it; merge-forward AV.1b → AV.4 before every
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
      under the same parallel load, same metrics.
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
      stated.

## Contract samples

Indicative `baselines.json` shape for the new families (D4); exact keys
follow the existing send-message-benchmark entry conventions:

```json
{
  "read-fanout": {
    "rand-m4.local": { "p50_floor_msgs_per_sec": 0, "seeded_runs": 3 }
  },
  "query-fts": {
    "rand-m4.local": { "p50_floor_msgs_per_sec": 0, "seeded_runs": 3 }
  },
  "read-under-write-load": {
    "rand-m4.local": { "p50_floor_msgs_per_sec": 0, "seeded_runs": 3 }
  }
}
```

Floor values are seeded from 3 clean runs (never hand-set); `0` above is a
placeholder illustrating shape only.

## Acceptance criteria

This is the authoritative acceptance checklist.

- [ ] A1 — `just benchmark` (or the family-specific target) runs all
      three families end-to-end on a dedicated benchmark account and
      publishes through the existing manifest/report contract; partial
      artifacts are invalid.
- [ ] A2 — Mixed-mode campaign demonstrates reads meeting budget while
      the writer sustains load; the report shows both read latency and
      concurrent write throughput.
- [ ] A3 — Floors are seeded from 3 clean runs on the reference host and
      committed to `baselines.json`; a synthetic serialization
      regression (e.g. pool size forced to 1 in a scratch build) fails
      the campaign (demonstrated once, then reverted).
- [ ] A4 — Official evidence only from isolated non-interactive
      accounts (m5-atmbench); no interactive-account campaign is cited.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`, `just test` (harness unit tests)
- [ ] One full campaign on the reference Mac host with committed
      artifacts under `site/reports/`
- [ ] `just reports-index --check`
- [ ] A3 scratch-regression demonstration recorded in sprint QA history.

## Out of scope

- Changing send-message-benchmark floors or harness behavior.
- Windows read-benchmark campaigns (follows the existing deferred
  Windows evidence policy; revisit after the send-path Windows work).
- Fixing the suspected batch-and-wait pipelining issue in the send
  harness (separately tracked, post-AO2).

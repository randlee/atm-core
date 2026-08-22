---
adr: ADR-054
title: Benchmark Report Finalizer Trust Model
status: Proposed
date: 2026-08-22
---

# ADR-054 — Benchmark Report Finalizer Trust Model

## Context

AO2.9 publishes physical benchmark evidence from a dedicated local account.
The account can observe the benchmark process and repository, but there is no
independent measurement verifier in this sprint. Publication is nevertheless
publicly consequential: a result becomes discoverable through the Pages branch
only after an evidence-branch commit, push, and reviewed pull request. The
implementation therefore needs an explicit trust boundary rather than an
implicit assumption that a local script is authoritative.

## Decision

1. `scripts/smoke/benchmark_publication.py` is the sole publication seam. Its
   `begin_run` function pushes the pre-execution intent and pending marker;
   `finalize_run` is the only function permitted to classify the outcome, write
   the immutable result/envelope/report/index, and push the publication.
2. The finalizer accepts only the canonical
   `evidence/ao2-benchmark-reports` branch with an open pull request into the
   Pages publisher branch. A local branch or an unreviewed push is evidence,
   not public publication.
3. The local benchmark account and finalizer are trusted to capture and
   classify their own measurement. The trust is bounded by the pushed intent,
   immutable result path, source and binary hashes, raw evidence, schema/path
   validation, generated-index checks, branch protection, and pull-request
   review.
4. AO2.9 does not add an independent verifier. A future verifier may compare
   raw evidence and recompute classifications in a separate reviewed sprint;
   its absence is recorded as residual risk rather than hidden behind a
   stronger status claim.

## Consequences

- A crash, power loss, or failed push leaves a durable intent/lock that exposes
  an `INCOMPLETE` attempt and prevents the next target from silently proceeding.
- Reviewers can distinguish what the local account measured from what the
  repository accepted as published, and can inspect the complete evidence
  lineage before merging the Pages PR.
- The local operator remains a trust anchor for measurement truth; hashes and
  raw artifacts improve auditability but cannot prove that a compromised
  account measured the intended workload.

## Residual risk and alternatives

The accepted residual risk is a compromised or mistaken local account
falsifying metrics or raw evidence before the finalizer pushes. Requiring a
remote measurement service or an independent verifier now would add a new
operational dependency and delay the isolated AO2 benchmark lane. That
alternative is explicitly deferred, not claimed to be equivalent.

## Required evidence

Implementation review must show the single-writer boundary, intent-before-run
ordering, failed-push lock behavior, immutable result/schema validation, raw
evidence retention, and the reviewed PR that makes the report reachable from
the Pages publisher. Quality review must cite this ADR when accepting the
local-finalizer trust model.

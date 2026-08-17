---
title: AQ.2 Search Projection Performance and Recovery Evidence
status: planned
branch: feature/aq2-search-projection-performance-evidence
target: integrate/phase-aq
worktree: ../atm-core-worktrees/feature/aq2-search-projection-performance-evidence
external_blockers: []
---

# AQ.2 — Search Projection Performance and Recovery Evidence

**recommended_agent:** arch-ctm/deep-reasoning (same-host performance and
durability evidence).
**must_follow:** AQ.1 pushed to `integrate/phase-aq`; merge it before every
development/fix round. AQ.1 owns the runtime behavior being measured, so a
pre-merge snapshot is invalid evidence.
**unblocks:** Phase AQ release decision for the AN.5 admission-performance regression.
**parallel_safe:** no production implementation sprint. Documentation-only
benchmark report work is parallel-safe after AQ.1 has a pinned candidate.

**traceability:** ADR-050; `REQ-P-SEARCH-INDEX-001`,
`REQ-RUSQLITE-SEARCH-INDEX-001`, `REQ-P-SMOKE-001`, `REQ-P-TEST-001`, and
`REQ-P-PLATFORM-002`.

## Deliverables

1. Retain a reproducible managed-daemon M5 benchmark report comparing the
   exact AQ.1 candidate against the historical FTS-free control
   `3b67fea40`, using the managed recovery/isolation procedure. The run uses
   a disposable benchmark state root, restores the operator database, and
   leaves the approved candidate daemon available for dogfooding unless a
   correctness defect or release-blocking regression requires explicit
   rollback.

2. Run the direct and F8 profiles with the same payload/profile definitions as
   the retained control. The candidate passes only if it reaches at least
   30,014 direct messages/second and 27,893 F8 messages/second (90% of the
   control). Capture throughput, latency distribution, pending-work high-water
   mark, drain count/duration, the exact source commit/binary paths, database
   isolation/restore proof, and daemon doctor status before/after.

3. Prove recovery under realistic lifecycle events: pending work survives a
   managed daemon restart; an idle drain reaches zero backlog; reindex yields
   the same projection; foreground admission remains responsive while a large
   backlog drains; and an intentional drain failure leaves durable work for
   later retry without corrupting canonical data. Keep a bounded cross-host
   smoke solely to prove normal message delivery remains unaffected; the
   search status remains local-only.

4. Add regression tests/artifacts for any benchmark-harness defect found
   during the campaign. Reports label an unexpected lower result as
   `regression`, not pass-by-absolute-threshold. A reproducible correctness
   defect receives a normal triage record, owner, fix, and rerun; this sprint
   cannot close while one remains unresolved.

## Acceptance criteria

- The retained M5 report proves the candidate's direct and F8 rates meet both
  exact floors on the same benchmark process/hardware as the control. A report
  lacking commit, isolation/restore proof, profile definition, or status
  metrics is invalid.
- All recovery vectors converge to current canonical rows; a backlog never
  changes canonical message visibility, routing, acknowledgement, or
  cross-host delivery.
- The report separates a design performance result from a correctness result.
  It does not claim a bug without evidence, nor call a result passing based on
  a reduced absolute target.
- Normal benchmark completion restores the user database and leaves the
  selected candidate daemon running for dogfooding. A real bug or confirmed
  material regression invokes the documented pre-benchmark-pair rollback.

## Required validation

- `just test`, `just lint`, targeted queue/recovery tests, managed M5 direct
  and F8 benchmark profiles, managed restart/recovery smoke, and a fresh
  local/cross-host Tokio+Axum smoke through the candidate daemon.
- Retain reports under `site/reports/send-message-benchmark/` and link the
  exact report filenames, source commit, baseline commit, and final CI commit
  from Phase AQ validation evidence.

## Paths to delete

None.

## Non-closure

AQ.2 does not retune unrelated SQLite/WAL settings, establish a new generic
job framework, change query grammar, or introduce remote analytics. Any
future user-selectable freshness policy requires a separate ADR and sprint.

---
phase: AO2
sprint: AO2.9
title: Benchmark report template, publication, and aggregate procedure
status: superseded
superseded_by: AO2.10, AO2.11, AO2.12, AO2.13
superseded_at: 2026-08-23
---

# AO2.9 — Benchmark report template, publication, and aggregate procedure (SUPERSEDED)

**Status: superseded.** This sprint's design (originally on branch
`plan/ao2-9-benchmark-report-template`, PR #989) is replaced in full by the
AO2.10-AO2.13 benchmark-report sprint plan in this worktree
(`docs/plans/phase-ao2/{benchmark-process-audit.md, benchmark-reporting-plan-overview.md,
sprint-AO2-10-benchmark-data-contract.md, sprint-AO2-11-benchmark-rendering-pipeline.md,
sprint-AO2-12-historical-data-migration.md, sprint-AO2-13-benchmark-run-skill.md}`).

PR #989 was closed (not merged) by explicit operator direction on 2026-08-23.
See `benchmark-reporting-plan-overview.md` for the full rationale: AO2.9's
two-phase git-lock publication protocol (pre-run `intent.json` commit+push,
`.pending/<run_id>.json` remote lock, push-gated finalizer, host-manifest
binding digests, evidence branch + PR gates per ADR-054) is replaced by a
simpler contract — one JSON array per run validated against a Pydantic model,
rendered via sc-compose, aggregated into a phase-level report.

Kept from the AO2.9 design and carried forward into the new plan: immutable
per-run result JSON; machine-classified PASS/FAIL/INCOMPLETE (never
caller-selected); OS-specific target matrix (Windows has no UDS); permanent
retention of failed runs; sc-compose as the sole rendering seam; extending
(not replacing) `.just/generate_report_index.py`.

Do not resume work against this sprint. Any residual scope belongs to
AO2.10-AO2.13.

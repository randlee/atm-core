---
title: AN.15 Checked-Render Adversarial Assurance
status: blocked
branch: feature/an15-adversarial-fuzzing
target: integrate/phase-an
external_blockers:
  - sc-sha, sc-composer, and sc-compose 1.4.1 published on crates.io
  - https://github.com/randlee/sc-compose/issues/448 closed
---

# AN.15 — Checked-Render Adversarial Assurance

**recommended_agent:** arch-ctm/deep-reasoning (template lifecycle and
negative-contract assurance).
**must_follow:** AN.14 merged. Before every dev/fix round, merge AN.14's
pushed integration tip because this sprint tests the exact checked-emission
routes and durable format catalog AN.13–AN.14 introduce. AN.15 is also
**blocked** until `sc-sha`, `sc-composer`, and sc-compose 1.4.1 are published
on crates.io and [sc-compose #448](https://github.com/randlee/sc-compose/issues/448)
is closed with direct-library checked-emission coverage. Never substitute an
unpublished git revision, path dependency, prerelease, or version range.

**unblocks:** checked-render assurance evidence for Phase AN close-out.
**parallel_safe:** none. The campaign exercises the complete AN.13–AN.14
template admission and rendering contract, and any confirmed regression must
be fixed and re-run against that exact integrated line.

**traceability:** AN.13, AN.14, Phase AN Decisions 1–3, 8, and 11–12;
ADR-036; [ADR-046](../../adr/ADR-046-template-declared-workflow-metadata.md);
and [the adversarial-fuzzing skill](../../../.claude/skills/adversarial-fuzzing/SKILL.md).

## Deliverables

1. Extend the repository's `adversarial-fuzzing` campaign contract and its
   validator tests with one explicit `atm-template-checked-emission` target.
   The target is restricted to an approved ATM worktree and operates through
   the public/deployed test seams for the `atm-template-sc-compose` adapter,
   catalog admission, and render-on-read/send paths. Keep the existing
   deterministic campaign envelope, maximum four workers, bounded cases,
   per-worker timeout, correlation IDs, minimization, three-reproduction
   rule, and durable report. The extension must not shell out from ATM to
   sc-compose, bypass the sealed adapter, or allow a worker to edit production
   code during a campaign.

2. Add a deterministic, realistic checked-render scenario corpus and its
   owning regression tests. The corpus must include all of the following:

   - **Captured environment snapshot.** A template resolved with an approved
     environment input such as `ATM_TEAM`, absent from the explicit JSON
     object, stores the fully merged value at admission. Remove or alter that
     process environment before render-on-read and prove the emitted bytes are
     unchanged. If the required value is unavailable at admission, reject
     before catalog/message mutation; a read path must never consult ambient
     process environment.
   - **Immutable revision and metadata snapshot.** Starting from an admitted
     template with frontmatter metadata/tags/defaults, make a one-byte body or
     frontmatter edit and admit it. The new raw bytes produce a distinct SHA
     and a catalog row containing the metadata parsed from *that exact edited
     file*. An unchanged frontmatter remains present on the new revision;
     removing or changing it is visible on the new revision and is never
     inherited from the old SHA. Re-admitting identical edited bytes is
     idempotent; the original row and its metadata remain unchanged.
   - **Checked-emission failures.** Malformed JSON, auto/legacy escape modes,
     incomplete final-body assembly (including a later guidance/prompt pass
     when applicable), Unicode/escaping, and confined include/import attempts
     exercise file-backed send, stored render-on-read, and verified rendered
     fallback. A rejected vector yields the documented typed error and proves
     no send, catalog/message mutation, cache/export body, or rendered value
     leak.
   - **Legacy and restart behavior.** Legacy/unverified catalog rows retain
     AN.13's compatibility state; they are never relabelled by the campaign.
     A newly re-registered classified revision survives reopen/restart and is
     checked deterministically from only persisted template bytes, metadata,
     output format, and merged variables.

3. Run and retain one bounded four-worker campaign using the updated skill:
   `shape-probe`, `template-probe`, `boundary-probe`, and
   `differential-probe`. Each worker runs at least 100 seeded cases with a
   120-second timeout and returns the existing structured JSON envelope.
   The differential worker compares a valid classified corpus with the AN.14
   baseline behavior where equivalent behavior is defined; malformed JSON is
   expected to differ only by the documented new rejection. Preserve seed,
   campaign ID, exact worktree/ref, worker results, minimized candidates,
   diagnostic codes, and final CI commit under the repository's normal fuzz
   report evidence path.

## Acceptance criteria

- The campaign validator accepts `atm-template-checked-emission`, rejects
  unsafe worktrees and unknown values fail-closed, and has deterministic
  unit coverage for worker selection and its contract fields.
- Every scenario in Deliverable 2 has an owning deterministic test before the
  campaign runs. The tests prove the specific no-mutation/no-leak and
  no-ambient-environment properties, not merely an error string.
- All four workers complete without a panic, hang, malformed envelope, or
  unclassified timeout. Every candidate is reproduced and minimized; a
  confirmed product bug has three reproductions and a durable owning-crate
  regression test. Intentional rejection cases are retained as negative
  contract coverage rather than filed as defects.
- The final report records `confirmed_bug`, `intentional_boundary`, or
  `inconclusive` for every candidate. AN.15 cannot close with an unresolved
  confirmed bug, a missing owner for an inconclusive safety candidate, or a
  campaign executed against an unpinned/pre-release upstream dependency.
- ATM records immutable facts only: SHA, exact parsed metadata/tag snapshot,
  output format, and merged variables. It does not add a parent-SHA,
  derivative heuristic, approval state, or required tag vocabulary. A
  template repository may independently lint protected metadata or expected
  tags; such policy is explicitly outside this ATM sprint.

## Required validation

- Targeted owning-crate tests for template admission, catalog persistence,
  render-on-read, and verified fallback; `.just/tests/test_run_fuzz.py` for
  the campaign-contract extension.
- One fresh four-worker `atm-template-checked-emission` campaign with the
  required corpus and retained JSON evidence, followed by reproduction of
  every candidate and promotion of every confirmed regression test.
- `cargo test -p atm-template-sc-compose -p atm-core -p atm-storage-rusqlite`,
  `cargo test -p atm-architecture --test boundary_enforcement`, `just lint`,
  and `just test` on Linux, macOS, and Windows CI.
- Verify the exact 1.4.1 crates.io sources/checksums and closed sc-compose
  #448 before campaign execution; retain that upstream evidence alongside the
  report.

## Paths to delete

Delete generated temporary templates, var files, and worker logs after the
campaign. Preserve only minimized reproducers, promoted tests, and the
sanitized durable report. Do not delete historical template revisions,
AN.13 legacy rows, or prior fuzz evidence.

## Non-closure

AN.15 does not implement template approval, template lineage, parent-SHA
links, protected-frontmatter enforcement, expected-tag policy, cross-host
template synchronization, or a new sc-compose API. Those are either
template-repository policy, synaptic-canvas-dolt provenance work, or upstream
sc-compose scope. The campaign may expose missing policy evidence, but ATM
must report those facts rather than invent a workflow-specific rule.

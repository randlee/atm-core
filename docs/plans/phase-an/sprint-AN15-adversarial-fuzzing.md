---
title: AN.15 Checked-Render Adversarial Assurance
status: draft
branch: feature/an15-adversarial-fuzzing
target: integrate/phase-an
---

# AN.15 — Checked-Render Adversarial Assurance

**recommended_agent:** arch-ctm/deep-reasoning (template lifecycle and
negative-contract assurance).
**must_follow:** AN.14 merged. Merged into `integrate/phase-an` at
`35108bc4be054fd52ce803c279a053011ed54513` (PR #877). Before every dev/fix
round, merge AN.14's pushed integration tip because this sprint tests the
exact checked-emission routes and durable format catalog AN.13–AN.14
introduce. AN.15's prior external blockers are satisfied: crates.io publishes
`sc-sha` **1.4.1**, `sc-composer` **1.4.1**, and `sc-compose` **1.4.1**; the
published `sc-composer` release exports `check_rendered_output`,
`CheckedOutput`, and `OutputFormat`; and
[sc-compose #448](https://github.com/randlee/sc-compose/issues/448) is
closed with direct-library checked-emission coverage. Never substitute an
unpublished git revision, path dependency, prerelease, or version range.

**unblocks:** checked-render assurance evidence for Phase AN close-out.
**parallel_safe:** none. The campaign exercises the complete AN.13–AN.14
template admission and rendering contract, and any confirmed regression must
be fixed and re-run against that exact integrated line.

**traceability:** AN.13, AN.14, Phase AN Decisions 1–3 and 8; ADR-036; and
[the adversarial-fuzzing skill](../../../.claude/skills/adversarial-fuzzing/SKILL.md).

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

   The target addition is deliberately a contract extension, not a second
   renderer. Its public shape and worker-to-boundary mapping are:

   ```python
   TARGETS = (
       # existing targets ...
       "atm-template-checked-emission",
   )
   ```

   | Worker | ATM seam exercised | Required oracle |
   | --- | --- | --- |
   | `shape-probe` | `TemplateComposer` input resolution into same-host template admission | merged variables are bounded/captured; malformed input cannot mutate catalog/message rows |
   | `template-probe` | sealed `atm-template-sc-compose` inspection and checked final rendering | format classification, escaping, Unicode, and checked body/error are deterministic |
   | `boundary-probe` | `TemplateCatalogStore` admission and root-confined file/fallback path | no include escape, partial catalog/message mutation, cache/export body, or value leak follows rejection |
   | `differential-probe` | `atm-core` render-on-read through the persisted catalog record | persisted bytes/format/merged variables, not ambient environment or a predecessor revision, determine output |

   A campaign worker may create only temporary inputs and structured evidence.
   When it confirms a product defect, the campaign adds the smallest durable
   owning-crate regression test but never a production fix. It records the
   exact seed/reproducer and owning seam (`atm-template-sc-compose`,
   `atm-core`, or `atm-storage-rusqlite`) in a canonical triage record at
   `.triage/phase-AN/findings/<finding_id>.ttl`, rendered through the
   [`triaging-findings`](../../../.claude/skills/triaging-findings/SKILL.md)
   workflow. Team-lead triages and dispatches that record by its
   Blocking/Important/Minor severity, exactly as for any QA finding. The
   assigned narrow follow-up PR contains the production fix; AN.15 then
   re-runs the minimized reproducer and full campaign on the merged fix. The
   no-edit rule applies only to live campaign workers, never to this explicitly
   separate fix-and-regression cycle.

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
     template with frontmatter metadata/defaults, make a one-byte body or
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
   baseline behavior for the AN.14 acceptance-defined equivalent corpus:
   valid text remains byte-identical and valid JSON succeeds. Malformed JSON
   is expected to differ only by the documented new rejection. Preserve seed,
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
  `inconclusive` for every candidate. A confirmed defect follows the separate
  owner/fix/retest cycle in Deliverable 1. An inconclusive safety candidate
  names its next investigator and is recorded in the report's existing
  `requirement_follow_up` field. AN.15 cannot close with an unresolved
  confirmed bug, unassigned safety investigation, or a campaign executed
  against an unpinned/pre-release upstream dependency.
- ATM records immutable facts only: SHA, exact parsed frontmatter metadata,
  output format, and merged variables. The repository-policy exclusions are
  defined once under Non-closure.

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

# Phase integration post-mortem

Required after sprint and fix layers land on the phase root's `integration_branch`,
before phase closure or the merge to the base branch. This replaces the TTL finding inventory
from `triaging-findings`: beads and their evidence are the source of truth.
It is part of the phase-end review, not another review of every fix layer.

## Inventory and target

Pin the fetched integration head. Record the branch and full commit SHA in
the report; inspect source and run focused checks from that commit.

Read all phase finding beads, including closed ones, without the default
50-row limit:

```bash
bd list --all -l phase-<x> -l stage:finding -n 0 --json
# Also inventory the full database: labels alone miss historical findings.
bd list --all -n 0 --json
```

From the full list, select the phase root's descendants and historical
finding IDs associated with the phase in reports; do not treat an ID pattern
alone as authoritative phase membership. Union and deduplicate by bead id.
Reconcile this list with the phase's QA, sanity and phase-end review reports
and the phase root's descendants. Include nested fix-on-fix findings and
legacy findings missing the label; account for every reported finding by bead
id. Use `bd show <id> --json` for resolution notes and linked evidence. A
missing bead, missing report, or unexplained inventory mismatch is a gap,
not a reason to silently reduce the reviewed set.

## Verify every resolution

- **Fixed:** read the original defect and remedy, fix receipt and verification
  evidence. Check the affected source and the defect's reproducer or focused
  regression check at the integration SHA, including fixes outside the
  original changed files. Record the check and result. Commit ancestry or a
  merged PR helps locate the fix but does not prove it survived later changes.
  For rebased/squashed fixes, verify equivalent source and behavior rather than
  requiring the original fix SHA to remain an ancestor.
- **Non-fix closure:** verify the recorded reason (`not_reproducible`,
  `not_applicable`, `ceremony`, duplicate, or an explicitly authorized
  deferral). Recheck factual claims against integration source. A duplicate
  must point to a finding whose disposition is also accounted for; a deferral
  must cite its authorization and follow-up owner/bead. Do not infer permission
  to defer from severity, closed status, or this reference.
- **Open, regressed, absent, or unverified:** report the exact bead and missing
  evidence. The lead routes it back to its owner; the reviewer does not silently
  close findings or invent a successful resolution.

This reconciliation verifies carried findings. It does not dispatch baseline
reviewers to discover replacements or expand each fix's scope. The phase-end
code review remains separate within the same report.

## Handling history and large inventories

Use ancestry, stable patch IDs (`git show <fix> | git patch-id --stable`),
PR/squash receipts, subjects and changed paths to locate an equivalent change.
Follow renames with `git log --follow -- <path>` when needed. None of these,
nor a percentage of matching added lines, proves the defect is fixed now.
Read the current affected source and focused check before marking verified.
Record the repository for every SHA. For another repository, use its relevant
integration source and receipt; do not classify its SHA as missing here.

Use JEV to screen straightforward fixed-code determinations, following
[post-mortem-jev.md](post-mortem-jev.md). Quality-mgr prepares the evidence,
checks coverage, and investigates unsure or flagged results before filing
beads or reporting defects to the lead. JEV results are not finding beads.
When delegation is authorized, workers may prepare disjoint evidence packets;
quality-mgr reconciles their IDs against the entire inventory. An ancestor SHA
still requires current-source verification. Sampling cannot produce a phase
PASS; an incomplete run retains its unchecked IDs as unresolved.

For administrative closures or evidence held outside git, read the bead's
notes and filing-reviewer receipt, or request that receipt from the owner.
Check rulings/deferrals against their actual authorization before treating the
original defect as actionable. Do not reopen work merely because an authorized
non-fix leaves the original code unchanged. Missing receipts remain unresolved.

## Report and closure

In `review-complete.md.j2`, set `integration_commit` to the same full SHA as
`commit`, and `post_mortem_counts` to nonnegative integer fields `total`,
`verified_fixed`, `justified_nonfix`, `unresolved`. Supply `post_mortem_md` containing:

- inventory total and counts verified fixed, justified non-fix, and unresolved;
  the counts must sum to the inventory total;
- one row per finding: bead id, recorded disposition, integration evidence
  (source/check/result or justified non-fix reason), and verified/unresolved;
- repeated finding families, their root causes, and the smallest useful
  prevention action with owner and target artifact, or `no_systemic_followup`.
  Do not manufacture a lint, ADR, or new process for every family.

Before the close, run `python3 .claude/skills/atm-bd-orchestration/scripts/check-review-completion.py <vars.json>`.
It rejects inconsistent verdicts, PASS with blocking/important code findings,
SHA mismatches and count totals; it does not
prove the prose evidence is true. The reviewer remains responsible for that.
Both the overall `verdict` and `integration_review` must fail if the audit is
incomplete or the accompanying code review has failed.

Set `integration_review` to `integration_review_passed` only when every
finding is accounted for and verified or explicitly authorized as a non-fix
closure. Otherwise use `integration_review_failed`, even if no new code
findings were filed. Zero findings still requires an explicit empty inventory.

After corrections land, refresh the inventory and verify affected findings
at the new integration head; carry forward unaffected evidence only after
checking intervening changes cannot invalidate it. A stale commit, unverified
finding, or inventory gap cannot authorize phase closure. The lead confirms
the report SHA equals the integration head before closing the root or merging
to the base branch.

---
title: AK.16 Phase-AK sprint-plan hardening and cleanup
status: deferred_pending_AK14_acceptance
branch: feature/pak-s16-phase-plan-hardening
worktree: ../atm-core-worktrees/feature/pak-s16-phase-plan-hardening
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.14 merged to integrate/phase-ak
merge_gate: AK.14 PR merge; zero unresolved Blocking/Important AK.16 findings; if optional replay is implemented, AK.17 PR merge and physical acceptance evidence
parallel_safe: false
quality_findings: []
---

# AK.16 — Phase-AK sprint-plan hardening and cleanup

## Closure

After AK.14 completes, review every Phase-AK sprint document against
`.claude/skills/plan-hardening/sprint-planning-guidelines.md` and correct every
structural violation in the plan documents before declaring Phase AK closed.
This is a documentation-quality and QA-routing sprint. It does not change a
runtime contract, reopen delivered implementation, or manufacture a passing
closure for a missing physical proof.

## Authoritative deliverables

1. Inventory every `docs/plans/phase-ak/sprint-AK*.md` document, including
   historical, superseded, completed, and deferred sprints. Record its status,
   dependency, owner recommendation, closure claim, and whether it is still
   dispatchable.
2. For each dispatchable sprint, verify it has exactly one authoritative list
   each for deliverables, acceptance criteria, deletion paths when applicable,
   and required validation. Consolidate duplicate or conflicting checklists.
3. Apply the guideline's split-early test to every non-completed sprint. Split
   an overloaded sprint before leaving AK.16 if its deliverables cannot land
   production-ready together, it mixes unrelated closure types, or its
   acceptance lets one deliverable slip while claiming success. Update all
   `must_follow`, merge gates, and scope references when splitting.
4. Verify every material trait, enum, protocol form, interface, and boundary
   contract has a precise signature or code sample where prose leaves an
   implementation choice open. Add only the minimum sample needed to remove
   ambiguity.
5. Reclassify all plan findings as `Blocking`/`Important` structural findings
   or `minor_wording`; do not bury a missing runtime/boundary/acceptance gate
   in wording cleanup.
6. Produce `docs/plans/phase-ak/AK16-plan-hardening-review.md` containing the
   inventory, each finding, its classification, exact document/line change,
   disposition, and the final dependency graph. The review is a navigation
   artifact; the corrected sprint document remains authoritative.

## Acceptance criteria

- `req-qa`, `arch-qa`, and `quality-mgr` can enumerate each dispatchable
  sprint's deliverables, acceptance criteria, paths to delete, and validation
  directly from that sprint doc without a downstream prompt narrowing scope.
- No dispatchable sprint has duplicate conflicting checklists, an unresolved
  structural finding, a missing dependency rationale, or a silent carry-forward
  of a claimed deliverable.
- Every `parallel_safe` declaration proves non-overlapping modules, public
  contracts, artifacts, and ownership; otherwise the relationship is
  `must_follow` with the merge-forward/PR-completion trigger stated.
- Historical and superseded sprints clearly say whether they are records only
  or may be dispatched. No superseded sprint is runnable.
- The final review reports every exception explicitly; AK.16 cannot claim
  minimal-phase completion while an AK.13 physical proof or runtime
  deliverable is still open. If AK.15 was approved, AK.16 also cannot claim
  optional-replay completion while AK.17 evidence is open.
- Any `Blocking` or `Important` finding discovered or carried forward by AK.16
  blocks both Phase-AK closure and the Phase-AK→`develop` merge PR until its
  owning implementation/evidence sprint is accepted. Only `minor_wording`
  items may be listed as non-blocking, and none may weaken an acceptance gate.

## Required validation

- Read and apply the full current sprint-planning guidelines at the start and
  include its repository path in the review artifact.
- Run link/reference checks for renamed, split, superseded, and deleted sprint
  files; verify every `must_follow`, `merge_gate`, finding ID, and plan-scope
  reference resolves.
- `just lint` and `just test` on the final documentation commit. If a
  documentation reference check is available, run it and record its command.
- Send the review artifact to req-qa, arch-qa, and quality-mgr for independent
  plan-only review. Fix returned structural findings in this sprint; list
  accepted wording-only findings separately.

## Explicit prohibitions

- Do not change code, test behavior, requirements, or ADR decisions merely to
  make a sprint document look complete.
- Do not reduce acceptance criteria, remove evidence requirements, or relabel
  a structural defect as wording to close a sprint.
- Do not re-enable replay, restore retired peer mechanisms, or weaken AK.12's
  boundary guards. Any runtime issue discovered is a new finding with a new
  implementation sprint.

## Dependencies and rationale

AK.16 must follow AK.14 PR completion to harden and close the minimal
AK.11–AK.14 sequence. Optional replay is not a phase-closure prerequisite:
if AK.15 is deferred or declined, AK.16 may complete from the minimal
baseline. If AK.15 is approved, AK.16 must instead follow AK.17 so it audits
the complete optional-replay evidence too. It is not parallel-safe with any
active Phase-AK implementation work: editing the authoritative plan during an
implementation round would create an ambiguous scope. After AK.16 merges,
the minimal phase may close only if its review shows no open structural plan
finding and all AK.13 evidence is accepted.

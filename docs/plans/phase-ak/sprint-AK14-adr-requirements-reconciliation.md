---
title: AK.14 ADR and requirements reconciliation
status: proposed
branch: feature/pak-s14-adr-requirements-reconciliation
worktree: ../atm-core-worktrees/feature/pak-s14-adr-requirements-reconciliation
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.13 merged to integrate/phase-ak
merge_gate: AK.13 merge commit
parallel_safe: false
quality_findings: [AK-MANDATE-ADR-DRIFT, AK-MANDATE-REQ-GAP]
---

# AK.14 — ADR and requirements reconciliation

## Closure

Closes F5 and F7 from
`docs/plans/phase-ak/phase-ak-mandate-compliance-fix-scope.md`. Root cause
of the whole phase-AK drift was that ADR-046/047 were written mid-phase,
codified divergences as if they were decisions, and QA then validated
against the ADRs instead of against the mandate. This sprint fixes the
documents to match the mandate as actually landed and proven by AK.11–AK.13, and adds
the process rule that prevents this class of drift recurring.

## Deliverables

1. **Retire ADR-046**, don't edit it in place. Its replacement record must
   restate the phase-AK cross-host constraint as a **literal checklist
   quoting §1 of the fix-scope doc verbatim** (the three SHALLs, the
   operator-confirmed SHALL #3 implementation clarification, and the
   default-off resend allowance) — not narrative prose. Mark ADR-046 itself
   `Status: Superseded`, pointing at the replacement.
2. **Amend ADR-047**: remove "the origin emits one ordered
   `PeerMessageArray`… a direct send is the one-element case" and any other
   text describing `messages[]` as the normal-path format. Replace with the
   actual AK.11–AK.13 baseline: one singleton write via the shared
   writer/reader; no production `messages[]` grammar or sender; and no
   automatic replay after network recovery. The checklist may state the
   original optional, default-off recovery allowance, but it must state that
   no such implementation exists in this baseline and that AK.15 requires
   separate operator approval.
3. Add **`REQ-CORE-TRANSPORT-002E`** to `docs/requirements.md` (not `002C`
   — already assigned to the unrelated same-host-proof requirement; current
   IDs in use: `002`, `002A`, `002B`, `002B1`, `002C`, `002D`). Text states
   the three SHALLs of §1, the operator-confirmed SHALL #3 implementation
   clarification, and the default-off resend allowance in checklist form,
   matching the ADR-046 replacement verbatim.
4. Document the **process rule** from fix-scope §5: any future ADR touching
   cross-host send must quote the relevant checklist verbatim in its own
   text, not paraphrase it, and req-qa/arch-qa must verify implementations
   against the quoted checklist — never against ADR narrative alone. Add
   this rule to wherever this repo's ADR process is documented (or create
   `docs/adr/README.md` process section if none exists).
5. Sweep global and package requirements/architecture/boundary documents
   (`docs/requirements.md`, `docs/atm-*/{requirements,architecture,boundaries}.md`
   where present, and `boundaries/**/*.toml`) for remaining references to
   `PeerMessageArray`, `PeerResendScheduler`, or default-on resend cache
   language left over from before AK.11–AK.13; correct or remove.

## Explicit prohibitions

- No new ADR in this sprint may relax any of the three SHALLs — this sprint
  reconciles documentation to match already-landed code, it does not design
  anything new.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs` (F0
  guardrail).
- Do not restate the checklist as prose anywhere it's supposed to be quoted
  verbatim — this is the exact failure mode being fixed.

## Required validation

- `just lint`, `just test` (doc-only sprint, but must not break doc-driven
  lints if any exist).
- req-qa and arch-qa must confirm they can verify AK.11–AK.13's actual code
  against the new `REQ-CORE-TRANSPORT-002E` checklist without needing ADR
  narrative.
- PR description links the AK.11, AK.12, and AK.13 merge commits and the
  AK.13 physical evidence bundle as the basis
  for the corrected documents.

## Dependencies

Starts after AK.13 merges, so the reconciled documents describe the final
state (ingress unified, dead grammar removed, mechanical gate in place, and
no-replay behavior physically proven). A later AK.15 may revise these
documents only as part of its explicitly approved default-off extension,
rather than an intermediate one. AK.16 hardens the minimal Phase-AK sprint
set after AK.14, or follows AK.17 if the optional replay path is approved.

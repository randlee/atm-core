# Phase Yb Post-Mortem

**Phase:** phase-Yb (Y.7 – Y.11)
**Integration branch:** integrate/phase-Y
**Final QA commit:** 9d5b3474
**Integration review result:** `integration_review_passed`
**Date:** 2026-05-17
**Author:** quality-mgr

---

## Finding Set Summary

| Metric | Count |
| --- | --- |
| Total findings triaged | 27 |
| Blocking | 8 |
| Important | 7 |
| Minor | 12 |
| Fixed | 23 |
| Waived | 1 (YB-026) |
| Deferred | 1 (YB-027) |
| Absent (no code occurrence) | 2 (YB-015, YB-022) |
| Repeatable | 1 (YB-019) |

All 8 blocking findings were closed before the final merge. PR #302 cleared the
0B+0I+0m gate at PYB-PHASE-END-QA-2.

---

## Finding Families

### Family A — Stale sprint / ADR status fields (DOC-001 pattern)

**Findings:** YB-007, YB-009, YB-011, YB-020, YB-023, YB-024
**Severities:** 2 blocking, 1 important, 3 minor

**Description:** Sprint doc files had `status: planned` on the integration
branch after the sprint completed. ADR-013 remained `Status: Proposed` despite
full implementation. Sprint paths were worktree-absolute instead of
repo-relative. Evidence mappings were prose without file:line columns.

**Root cause:** Sprint acceptance criteria do not require updating `status:`
frontmatter as a close-out step. No lint fires on `status: planned` in a merged
branch. Humans write the status line once, then forget it.

**Pattern recurrence:** YES — appeared in at least Y.7, Y.8, Y.10, Y.11.
Similar pattern observed in Phase W and Phase Xb.

**Classifications:**
- `qa_process_improvement` — QA checklist must verify status frontmatter is
  updated before issuing a sprint PASS
- `new_lint` — lint rule: sprint docs on integration branch must not carry
  `status: planned`; ADRs must carry `Status: Accepted` once implementation is
  merged

**Target artifacts:**
- `.just/run_lint.py` — add sprint-status lint rule
- `.claude/skills/quality-management-gh/SKILL.md` — add status-field check to
  sprint QA checklist
- `docs/phase-Yb/sprint-*.md` template — add closeout step: "update status
  field before pushing final commit"

---

### Family B — RULE-003 file-size violation

**Findings:** YB-001
**Severity:** 1 blocking

**Description:** `send/mod.rs` reached 1792 production-code lines. RULE-003
threshold is 1000.

**Root cause:** RULE-003 is documented and QA checks it, but no automated
gate fires during development. Violations accumulate sprint by sprint until QA
catches them.

**Pattern recurrence:** YES — similar violations in Phase Xb and Phase Y.

**Classifications:**
- `new_lint` — RULE-003 must be a CI-blocking lint gate, not just a QA check

**Target artifacts:**
- `.just/run_lint.py` or `sc-lint` — add per-crate non-test line-count check
  against RULE-003 threshold
- `boundaries/` TOML — add RULE-003 as a mandatory boundary rule that fails PRs

---

### Family C — Architecture boundary escapes

**Findings:** YB-002, YB-005, YB-008
**Severities:** 3 blocking (all ARCH)

**Description:**
- YB-002: `emit_delivery_transitions` branched on `plan.disposition` after the
  machine-owned seam — policy logic outside the state machine.
- YB-005: `maybe_run_post_send_hook` used `pub(crate)` without a named-caller
  allowlist entry, making the boundary undocumented.
- YB-008: `DaemonNonClaudeOutbound` was instantiated as a dead phantom field
  (RULE-004) — wired in composition but never injected into the delivery path.

**Root cause:** Boundary rules exist in prose (lintable-boundary-plan.md) but
machine enforcement (LINT-BOUNDARY-* gates) was not active until Y.10, and even
then was not complete. Developers can accidentally violate documented rules if
no CI gate rejects the commit.

**Pattern recurrence:** Partially — boundary escape patterns appeared in Phase
Y and Phase Xb. The specific delivery-seam escapes are new to Yb.

**Classifications:**
- `boundary_update` — complete the LINT-BOUNDARY-INBOX-EXPORT-REFERENCES and
  LINT-BOUNDARY-NON-CLAUDE-OUTBOUND-REFERENCES gates
- `new_lint` — dead-phantom detection (RULE-004): flag fields that are
  assigned in construction but never read outside tests

**Target artifacts:**
- `boundaries/non-claude-outbound.toml` — verify named-caller allowlist is
  enforced by CI
- `.just/run_lint.py` — complete LINT-BOUNDARY-* rule implementation (YB-015
  was absent; must be implemented or removed from the plan)
- `docs/phase-Yb/lintable-boundary-plan.md` §2 — mark rules as CI-active vs
  documented-only; remove claims for unimplemented gates

---

### Family D — Test coverage insufficient for delivery proof

**Findings:** YB-003, YB-004
**Severities:** 2 blocking (ATM-QA)

**Description:**
- YB-003: Test used `captures.len() == 2` (hook count) as non-Claude delivery
  proof. The lintable-boundary-plan explicitly forbids notification hooks as
  delivery proof.
- YB-004: Only fault-injection tests were present. No named success-path test
  for the SQLite + ClaudeCode flow.

**Root cause:** Sprint acceptance criteria name the degraded/fault paths but
do not explicitly require a named success-path test. The hook-as-proof pattern
is natural to write but violates the plan; the rule was documented in prose
but not enforced.

**Pattern recurrence:** YES — appeared in Y.7 sprint. Similar pattern
(notification-as-proof) appeared in Phase Y S4–S5.

**Classifications:**
- `sprint_plan_update` — every sprint with delivery tests must name both a
  success-path test and fault-injection tests in acceptance criteria
- `qa_process_improvement` — QA checklist must verify outbound payloads through
  the owning delivery boundary, not hook invocation count

**Target artifacts:**
- `docs/phase-Yb/sprint-Y*.md` template — acceptance criteria section must
  include `named_success_test:` and `named_fault_tests:` fields
- `.claude/skills/quality-management-gh/SKILL.md` — add check: "does the test
  suite include a named success-path test for each delivery harness family?"

---

### Family E — Boundary allowlist gaps

**Findings:** YB-006, YB-010, YB-014, YB-015, YB-025
**Severities:** 1 blocking, 2 important, 2 absent

**Description:**
- YB-006: `lintable-boundary-plan.md` was written without named caller
  allowlists despite Y.8 acceptance criteria requiring them.
- YB-010: Sprint-Y10 listed the wrong boundary TOMLs in its changed-files
  section.
- YB-014: Inbox-export boundary TOMLs were missing `allowed_callers` fields.
- YB-015: LINT-BOUNDARY-INBOX-EXPORT-REFERENCES was named in the plan but
  never implemented in `lint_boundaries.py`.
- YB-025: `daemon-non-claude-outbound.toml` claimed `DaemonNonClaudeOutbound`
  was wired as the delivery adapter when it was actually a phantom;
  `LocalFileNonClaudeOutbound` was the real production adapter.

**Root cause:** Boundary TOML files have no machine-enforced schema requiring
named-caller fields. Developers can omit or misstate the allowlist without a CI
failure. Plan documents claim lint gates that were not yet implemented.

**Pattern recurrence:** YES — first gap appeared in Y.8, recurred through Y.10
and Y.11.

**Classifications:**
- `boundary_update` — boundary TOML schema must require `allowed_callers` field
- `new_lint` — lint rule: boundary TOML files without `allowed_callers` block
  fail the lint gate
- `planning_process_improvement` — plan documents must not claim lint gate
  names unless the gate implementation is committed in the same sprint

**Target artifacts:**
- `boundaries/*.toml` schema — add required `allowed_callers` field
- `.just/run_lint.py` — add TOML schema validation step
- `docs/phase-Yb/lintable-boundary-plan.md` — add status column (active/planned)
  to the primitive caller allowlist table

---

### Family F — Missing `.with_recovery()` on error paths

**Findings:** YB-012, YB-013
**Severities:** 2 important (RBP)

**Description:** Two `AtmError` construction sites in `service_runtime.rs` and
`non_claude_outbound_runtime.rs` lacked `.with_recovery()`. Subsequent error
paths in the same files did have it, creating inconsistent error ergonomics.

**Root cause:** No automated check exists for `AtmError::*` construction
without a `.with_recovery()` call. The pattern is required by RBP but
enforced only through human code review and QA.

**Pattern recurrence:** YES — RBP findings for missing `.with_recovery()` have
appeared in multiple phases (Phase Y, Phase Xb, Phase W).

**Classifications:**
- `new_lint` — grep/AST rule: `AtmError::` constructor calls not followed by
  `.with_recovery()` within the same expression should be flagged

**Target artifacts:**
- `.just/run_lint.py` or `sc-lint` — add `missing_recovery_context` rule
- `.claude/agents/rust-best-practices-agent.md` — add explicit check for
  AtmError recovery context as a first-class RBP rule

---

### Family G — Absent / phantom findings

**Findings:** YB-015, YB-022
**Status:** absent (no code occurrence found)

**Description:**
- YB-015: `LINT-BOUNDARY-INBOX-EXPORT-REFERENCES` was named in the plan but
  triage found no implementation in `lint_boundaries.py`.
- YB-022: `#[expect(dead_code)]` reason referenced stale Y.6 instead of Yb
  Y.10 — but triage found no such occurrence in code.

**Root cause:** Triage created findings from plan documentation claims without
first verifying the claim against code. YB-015 is a real gap (plan overstates
what was built). YB-022 may have been fixed before triage ran.

**Classifications:**
- `qa_process_improvement` — before creating a finding for a named plan
  deliverable, triage must grep for the named symbol/file and confirm the claim
  against actual code; "absent" findings should be labeled as `plan-claim
  unverifiable` not as code violations

**Target artifacts:**
- `.claude/agents/qa-triage.md` — add pre-flight verification step: grep for
  named symbol before creating a code-violation finding

---

## Integration Review Summary

| Condition | Result |
| --- | --- |
| All sprint branches merged into integrate/phase-Y | YES (Y.7–Y.11, PRs #304–#308) |
| Final QA on integration branch | PASS at 9d5b3474 (PYB-PHASE-END-QA-2) |
| 0 blocking + 0 important + 0 minor remaining | YES |
| All blocking findings closed | YES (8/8) |
| Waived findings documented | YES (YB-026 — synchronous trait contract) |
| Deferred findings tracked | YES (YB-027 → post-Yb phase) |
| Absent findings investigated | YES (YB-015, YB-022 — no code occurrence) |
| Merge authorization | Pending team-lead (PR #302) |

**Integration review verdict:** `integration_review_passed`

---

## Systemic Recommendations Summary

| # | Finding family | Classification | Owner | Target artifact |
| --- | --- | --- | --- | --- |
| 1 | DOC-001 status fields (Family A) | `new_lint` | arch-ctm | `.just/run_lint.py` — sprint status lint rule |
| 2 | DOC-001 status fields (Family A) | `qa_process_improvement` | quality-mgr | `.claude/skills/quality-management-gh/SKILL.md` |
| 3 | RULE-003 file size (Family B) | `new_lint` | arch-ctm | `.just/run_lint.py` — per-crate line-count gate |
| 4 | Arch boundary escapes (Family C) | `boundary_update` | arch-ctm | `boundaries/*.toml` — complete LINT-BOUNDARY-* gates |
| 5 | Arch boundary escapes (Family C) | `new_lint` | arch-ctm | `.just/run_lint.py` — RULE-004 dead-phantom detection |
| 6 | Test coverage / proof surface (Family D) | `sprint_plan_update` | team-lead | Sprint plan template — named success + fault tests |
| 7 | Test coverage / proof surface (Family D) | `qa_process_improvement` | quality-mgr | QA checklist — verify payload proof surface |
| 8 | Boundary allowlist gaps (Family E) | `boundary_update` + `new_lint` | arch-ctm | `boundaries/*.toml` schema, lint step |
| 9 | Boundary allowlist gaps (Family E) | `planning_process_improvement` | team-lead | Sprint plan conventions — no claimed gates without implementation |
| 10 | Missing `.with_recovery()` (Family F) | `new_lint` | arch-ctm | `.just/run_lint.py` — AtmError recovery context rule |
| 11 | Absent findings (Family G) | `qa_process_improvement` | quality-mgr | `.claude/agents/qa-triage.md` — pre-flight grep verification |

---

## Decision Rationale

Preference order (smallest upstream control):

1. **New lint** (items 1, 3, 5, 8, 10) — mechanically blocks the defect class
   before QA
2. **Boundary enforcement** (items 4, 8) — structural ownership gates
3. **Process improvement** (items 2, 6, 7, 9, 11) — checklist and template
   changes where automation is not practical

No finding family was left with manual reviewer vigilance as the only control.
The two items relying on QA checklist changes (items 2, 7) are lower-frequency
edge cases where lint automation is impractical (prose wording quality, test
naming conventions).

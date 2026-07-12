# Historical Reserved Draft — sc-lint Dependency-Policy Ownership And Rollback Posture

| Field | Value |
|---|---|
| ID | Historical reserved draft; not an accepted ADR ID |
| Status | **Retired draft** |
| Date | 2026-07-02 |
| Deciders | superseded by later namespace reuse; no decision ever taken here |
| Placeholder | historical `AD9-PLACEHOLDER-NOT-ACCEPTED` reservation only |
| Relates to | `docs/plans/sc-lint-migration/plan.md` |
| Supersedes | — |
| Replaced by | accepted ATM ADR-019 at `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md` |

---

## Context

This file was created as a reserved draft while the sc-lint migration proposal
still used the provisional `AD.*` sprint namespace.

That sc-lint proposal never executed.

The `AD.*` namespace was later consumed by unrelated accepted ATM work. The
accepted line recorded in `docs/project-plan.md` and
`docs/plans/phase-AD/readiness.md` is caller-identity / post-send correction
work through `AD.35`, not sc-lint migration work.

This means:

- no sc-lint execution has occurred under any `AD.*` sprint
- the original `AD.9` decision point named here is permanently obsolete
- any future sc-lint execution line needs a new phase identifier plus explicit
  human authorization before it can claim an ADR-backed decision point
- this file is not part of the active ADR index and must not be treated as the
  accepted ADR-019 for ATM

## Current Disposition

Use [docs/plans/sc-lint-migration/plan.md](../plans/sc-lint-migration/plan.md)
as the source of truth for the current sc-lint migration inventory, namespace
collision disclosure, and future authorization gate.

If a future sc-lint execution phase needs an ADR for dependency-policy
ownership and rollback posture, author it under a new non-colliding ADR number
and reference the new phase identifier rather than reviving this draft.

No architectural decision is accepted in this file.

# ADR-019 — sc-lint Dependency-Policy Ownership And Rollback Posture

| Field | Value |
|---|---|
| ID | ADR-019 |
| Status | **Proposed** |
| Date | 2026-07-02 |
| Deciders | TBD during `AD.9` |
| Relates to | Phase `AD`, `docs/plans/phase-AD/sprint-AD9.md` |
| Supersedes | — |

---

## Context

Phase `AD` migrates ATM from vendored `sc-lint` crates to published
`sc-lint` releases. Sprint `AD.9` is the point where ATM decides whether the
released `Phase D.1` dependency-policy surface becomes the authoritative
enforcement path for ATM boundary records.

That decision cannot stay implicit because `AD.9` may:

- move dependency-policy ownership from ATM-local Python checks to published
  `sc-lint`
- delete or reduce duplicate ATM-local dependency-policy logic
- require rollback posture if the released analyzer proves incomplete or weaker
  than the current ATM-local coverage

The plan therefore reserves this ADR file up front so `AD.9` cannot claim
closure without a concrete repository record for the ownership and rollback
decision.

## Reserved Final Coverage

The final `AD.9` implementation must update this ADR to record:

- the exact published `sc-lint` version adopted for dependency-policy
  enforcement
- the dependency-policy ownership boundary between published `sc-lint` and any
  residual ATM-only governance checks
- the rollback posture if the published `sc-lint` dependency-policy release is
  insufficient
- the checkpoint or follow-on-phase decision if the required published
  analyzer release is unavailable when `AD.9` is ready to execute

## Placeholder Decision

This ADR number and file path are reserved now for the `AD.9` cutover.

No architectural decision is accepted yet beyond this planning requirement:

- `AD.9` must not close without an explicit ADR covering dependency-policy
  ownership and rollback posture for the published `sc-lint` cutover

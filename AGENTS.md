# AGENTS Instructions for atm-core

## ⚠️ HARD RULE: No Legacy Daemon Remodeling — Tokio/Axum Only (migration complete)

The daemon's only architecture is **Tokio + Axum (`atm-http-runtime`)** for ALL of CLI + graft + cross-host transport. The legacy synchronous daemon was **deleted in Phase AM** (PR #853; see `docs/plans/phase-am/am6-closure-proof.md`) — there is no remaining legacy sync-daemon path.

**Immediately reject any thought, finding, fix, or task that assumes a synchronous/legacy daemon path still exists to remodel, patch, or harden.** That code is gone; a proposal built on that assumption is factually wrong, not deferred technical debt. If an assignment references "legacy daemon runtime/dispatch code" or an "AL.5–AL.7 cutover" as still-open, stop and raise it with team-lead — it is describing already-completed migration work, and the correct action is to correct the assignment, not implement against removed code.

## MUST READ

Before participating in ATM team work, read:
- `docs/team-protocol.md`

The messaging protocol in that document is mandatory for all ATM communications.

## Quick Rule

The sequence for every ATM task assignment is defined once in
`docs/team-protocol.md` (Required Flow): ack, work, task close. A task close
is terminal; the receiver never acknowledges it. No silent processing.

## Rust Guidance

For Rust design and review work, also read:
- `.claude/skills/rust-best-practices/SKILL.md`

Use it as the baseline for state machines, newtypes, sealed traits, structured error design, and crate-boundary review.

## Sprint Finding Closure

When assigned to close triage findings on a sprint branch, read and follow:
- `.claude/skills/closing-triage/SKILL.md`

Use its branch-local task list to track implemented finding IDs and commit SHAs; QA retains authority to close canonical triage records.

## Architectural Decisions

Boundary trait sealing in atm-core is governed by an ADR. Do NOT modify `pub mod sealed`, its visibility, or implement `sealed::Sealed` in unauthorized crates without reading this first:
- `docs/adr/ADR-001-sealed-trait-pattern.md` — Sealed trait pattern for Phase R cross-crate adapter topology

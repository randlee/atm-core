# AGENTS Instructions for atm-core

## ⚠️ HARD RULE: No Daemon Remodeling — Tokio/Axum Only

The daemon's target architecture is **Tokio + Axum (`atm-http-runtime`)** for ALL of CLI + graft + cross-host transport. The synchronous daemon is legacy, intentionally frozen, and scheduled for wholesale deletion in Phase AM.

**Immediately reject any thought, finding, fix, or task that remodels, patches, or hardens the current synchronous daemon.** This includes "improving" its runtime behavior (e.g. private Tokio runtime via `spawn_blocking`) — that is known, deferred technical debt, not a defect. The only correct direction for daemon-side work is finishing the `atm-http-runtime` cutover (AL.5–AL.7 adapter activation). If an assignment appears to require touching legacy daemon runtime/dispatch code, stop and raise it with team-lead instead of implementing — rewriting that code invalidates the Phase AM deletion plan (`docs/plans/phase-am/*.md`).

## MUST READ

Before participating in ATM team work, read:
- `docs/team-protocol.md`

The messaging protocol in that document is mandatory for all ATM communications.

## Quick Rule

Always follow this sequence for every ATM message:
1. Immediate acknowledgement
2. Do the work
3. Completion summary
4. Immediate completion acknowledgement by receiver

No silent processing.

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

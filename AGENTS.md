# AGENTS Instructions for atm-core

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

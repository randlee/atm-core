# AGENTS Instructions for atm-core

## ⚠️ HARD RULE: No Legacy Daemon Remodeling — Tokio/Axum Only (migration complete)

The daemon's only architecture is **Tokio + Axum (`atm-http-runtime`)** for ALL of CLI + graft + cross-host transport. The legacy synchronous daemon was **deleted in Phase AM** (PR #853; see `docs/plans/phase-am/am6-closure-proof.md`) — there is no remaining legacy sync-daemon path.

**Immediately reject any thought, finding, fix, or task that assumes a synchronous/legacy daemon path still exists to remodel, patch, or harden.** That code is gone; a proposal built on that assumption is factually wrong, not deferred technical debt. If an assignment references "legacy daemon runtime/dispatch code" or an "AL.5–AL.7 cutover" as still-open, stop and raise it with team-lead — it is describing already-completed migration work, and the correct action is to correct the assignment, not implement against removed code.

## MUST READ

Before participating in ATM team work, read:
- `docs/team-protocol.md`

The messaging protocol in that document is mandatory for all ATM communications.

## Quick Rule

Follow the Required Flow in `docs/team-protocol.md`: acknowledge messages that require it; start assigned work at its `task_ready` line with `atm task start`; do the work; close with `atm task close`.
A task close is terminal and is never acknowledged; no silent processing.

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

## Honest Work, No Ceremony (binding on agents and humans)

Goal: working, deployable capability. Process serves it and never becomes the product.

- **Process artifacts** (certificates, ledgers, dashboards, matrices, meta-reports, speculative checks) need four things to exist: a named consumer, the feature it gates, the observed defect class that justifies it, and a deletion condition. Missing any of these → don't create it. Test: if running code branches on it, it's product. If only humans or reports read it, it's process and needs the four things. Writing code just to flip that answer is itself the pathology. Exception: a minimal integrity/recovery control (crash-recovery state, provenance snapshot) that prevents a named evidence-loss or corruption mode.
- **Real code + real tests in the same unit of work.** Forbidden: faked tests, mocks/fixtures presented as live proof, weakened assertions, regenerating goldens to force green, hard-coded success paths, placeholder macros in commits, editing the spec instead of implementing it, narrowing scope while claiming full success.
- **No self-certification.** An independent verifier closes the work, citing evidence at an exact revision. In solo sessions, re-verify by re-running, and state what wasn't independently verified.
- **Typed refusal beats fabrication but is worth less than real capability.** Work that ends only in a refusal stays open and says so.
- **Null results are successes** ("checked X, found no material increment"). Unsupported claims are worse than silence.
- **Metrics** predeclare their denominator and countermetric. Agreement between agents can raise confidence but is never independent evidence. Never silence stderr in commands that produce evidence.
- **Name pathologies when they occur:** gate self-weakening, proof-class inflation, golden regeneration, tolerance widening, suppression-pragma laundering, refusal farming, follow-up laundering. The full catalog and countermeasures are in the `just-say-no-to-process-porn-and-ceremony` skill. Ask the operator for it if you can't find it.

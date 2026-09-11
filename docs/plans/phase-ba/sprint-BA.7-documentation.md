# BA.7 — Documentation and CLAUDE.md correction

| Field | Value |
| --- | --- |
| Wave | 1 |
| Branch | `docs/ba7-task-nudge-documentation` |
| Base | `integrate/phase-ba` |
| Stack | not stacked — independent PR |
| Dependency | `parallel_safe` with BA.1, BA.2, BA.4 |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Goal

Close the documentation half of Rand's problem (b). Orchestrators interrupt
working agents partly because the non-interrupting path is undocumented and
the documentation actively recommends the interrupting one.

Documentation only. No code, no tests, no boundary manifests.

## The defect

- `atm send` defaults to `NudgeMode::Immediate` (`send/mod.rs:76-82`,
  `#[default] Immediate`), which is the Steer/interrupt path.
- `atm queue` is the Deferred path (`commands/queue.rs` runs
  `run_with_mode(..., NudgeMode::Deferred)`) and appears in **no**
  documentation — not `CLAUDE.md`, not `docs/agent-conventions.md`, not
  `docs/team-protocol.md`.
- `CLAUDE.md` instructs `atm send` for task assignment at lines 215 and 227
  and in the quick-reference table at line 234.

So the one existing mechanism that delivers without wrecking context is
invisible, and every orchestrator is steered into the interrupting path by the
project's own instructions.

## Deliverables

1. Document `atm queue` in `docs/agent-conventions.md` and
   `docs/team-protocol.md`: what deferred delivery means, when to use it
   versus `atm send`, and that it does not interrupt an agent mid-task.
2. Correct `CLAUDE.md` lines 215, 227 and the quick-reference table so task
   assignment does not default to the interrupting path.
3. Document the closed `atm task` command set as **planned surface**, clearly
   marked as not yet shipped, with a pointer to BA.5. Do not describe it as
   available.
4. Document the two meanings of blocked — agent `blocked` versus task
   `refused` — in `docs/agent-conventions.md`, so the vocabulary is fixed
   before BA.5 implements the outcomes.

## Affected paths

- `CLAUDE.md`
- `docs/agent-conventions.md`
- `docs/team-protocol.md`

## Paths that must not change

- every `crates/` path
- `docs/adr/*` — ADR-054 belongs to BA.2
- `docs/plans/phase-az/*` and `.triage/phase-az/*` — retained history

## Acceptance criteria

1. `atm queue` appears in both agent-facing documents with guidance on when to
   prefer it.
2. No remaining instruction in `CLAUDE.md` directs task assignment through the
   interrupting default. Gate: reviewer reads lines 200-240 and confirms.
3. The `atm task` surface is described as planned, with no wording implying it
   exists today.
4. `blocked` and `refused` are defined distinctly and neither word is used for
   the other concept.

## Required validation

- the documentation lint path, not full CI — this is a doc-only change
- `just lint` where it covers markdown

## Non-closure

This sprint documents `atm queue` as it exists today. It does **not**
implement ephemeral queued-message scheduling; that is BA.6. If BA.6 changes
observable behaviour, BA.6 owns the follow-up documentation edit.

**This sprint is not the final word on the `atm task` surface
(PLAN-CRIT-021).** It runs in wave 1, before R1 and R2 are exercised in code
and before BA.5 and BA.9 ship the verbs, so AC3 deliberately requires the
surface be described as *planned*. **BA.11** owns the replacement text once
the surface exists. Do not treat this sprint's wording as durable, and do not
pre-empt BA.11 by describing unshipped verbs in the present tense here.

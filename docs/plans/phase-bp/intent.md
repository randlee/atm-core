# Intent: phase-bp (bd-based plan hardening in atm-bd)

Jev judges against `anchors.json`. This page explains why.

## Problem

`/plan-hardening` exists only in atm-core. It reviews markdown sprint docs and passes fenced JSON between six steps that the team lead routes by hand. Plans are now written in beads, so the skill reviews files that no longer exist. Its instructions live in subagent prompts that nobody can query afterwards.

## What ships

A plan-hardening skill in the atm-bd package that works only on bd plans.

- **Instructions are beads.** Each iteration is a bd wisp (an ephemeral molecule) made from a formula. Each step bead's description is the full instruction for that pass. Wisps stay out of `bd ready`, so they never enter the lead's dispatch loop. When the run is done they are squashed, and the digest keeps each step's close reason.
- **One step template, two runners.** `plan-hardening-step.xml.j2` takes `runner = teammate | subagent`.
  - A teammate runs `atm task start`, claims the step bead, does the pass, closes the bead, and runs `atm task close`.
  - A subagent claims, does the pass, closes the bead, and returns `{step, verdict, findings}`. It runs no `atm` command.
- **Findings are fix beads.** Each finding is one bead assigned to the planner, labelled `stage:plan-fix`. The pass that filed it re-checks it in the next iteration and closes it once it is fixed.
- **Three modes:**
  - **lead adversarial** is the default whenever the team is idle. The lead attacks the plan itself and sends adversaries to cover external repositories, APIs, and REQ/ADR consistency.
  - **planner with teammates** is for hard plans that are worth interrupting another frontier model, preferably from a different foundry.
  - **planner with subagents** is for mid-phase replans while everyone is busy. It needs nothing from the lead.
- **Iterations.** There are 2 by default. Scope, critical, and adversary passes run in parallel within an iteration. ceremony-qa runs after the final critical pass. The planner is told that a final critical FAIL is normal.
- **Evals.** Evals cover haiku and luna, each as a subagent and as a teammate, against a seeded plan with planted defects. Each eval ends by asking the agent questions about the run.

## Not in this phase

- **atm-core switch-over.** atm-core deletes its markdown `.claude/skills/plan-hardening/` in the install PR for the tagged atm-bd release that ships this skill.
- **Orchestration code.** It stays untouched (shared-code ruling, 2026-11-01). The skill reuses the existing Plan Gate (`plan-review-template.xml.j2`) unchanged.
- **Guidelines.** atm-bd's `atm-beads-plan-guidelines.md` is already the bd form of atm-core's `sprint-planning-guidelines.md`. Every remaining difference is markdown-doc or atm-core specific, so no guideline text moves.

# Anchors - phase-bp

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bp/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Three run modes | 2026-11-03 |
| ANC-002 | locked | ceremony-qa after final critical pass | 2026-11-03 |
| ANC-003 | locked | Final critical FAIL is normal | 2026-11-03 |
| ANC-004 | locked | Default two iterations | 2026-11-03 |
| ANC-005 | locked | Passes in an iteration run in parallel | 2026-11-03 |
| ANC-006 | locked | Adversaries get new tasks in iteration 2 | 2026-11-03 |
| ANC-007 | locked | Phase lead files its own findings | 2026-11-03 |
| ANC-008 | locked | Research each external repo or API integration | 2026-11-03 |
| ANC-009 | locked | Review and revise related REQs and ADRs | 2026-11-03 |
| ANC-010 | locked | Researcher tier by difficulty, dev tiers | 2026-11-03 |
| ANC-011 | locked | Mix foundries, including across rounds | 2026-11-03 |
| ANC-012 | locked | Instructions are ephemeral beads | 2026-11-03 |
| ANC-013 | locked | One step template for both runners | 2026-11-03 |
| ANC-014 | locked | Teammate starts task; both claim and close bead | 2026-11-03 |
| ANC-015 | locked | Findings become fix beads for the planner | 2026-11-03 |
| ANC-016 | locked | bd only | 2026-11-03 |
| ANC-017 | locked | Installed as part of atm-bd | 2026-11-03 |
| ANC-018 | locked | Evals for haiku and luna, both runners | 2026-11-03 |
| ANC-019 | locked | Evals interrogate the agent after the run | 2026-11-03 |
| ANC-020 | locked | Idle team means adversarial mode | 2026-11-03 |
| ANC-021 | locked | Hard plan means planner with teammates | 2026-11-03 |
| ANC-022 | locked | Planner mode needs no lead | 2026-11-03 |

## ANC-001

Anchor: The atm-bd plan-hardening SKILL.md must define three run modes: planner with subagents, planner with teammates, and phase lead adversarial to the planner.

> /plan-hardening can be run by a planning agent w/ his subagents (if team-mates are busy) OR w/ team-mates OR orchestrated by phase-lead adversarially to planning-agent

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Three run modes

Scope: adds or changes the plan-hardening SKILL.md run modes or the step template

Deterministic check to pair with the development gate: grep -c the three mode names in plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md == 3 mode headings

Fixtures:
- pass: skills/plan-hardening/SKILL.md has a Run Modes section defining three modes: planner with subagents, planner with teammates, and lead adversarial, where the phase lead orchestrates against the planner (test_skill_names_three_modes).
- fail: skills/plan-hardening/SKILL.md describes one run mode: the team lead routes every pass to reviewers.

## ANC-002

Anchor: In the atm-bd plan-hardening formulas, the ceremony-qa review step must depend on the step for the final critical-plan-reviewer pass, so ceremony-qa reviews the plan after that pass.

> ceremony-qa agent must review the plan after final @critical-plan-reviewer pass (too much critical review can cause ceremony)

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: ceremony-qa after final critical pass

Scope: adds or changes the plan-hardening formulas, step order or SKILL.md flow

Deterministic check to pair with the development gate: in plugins/atm-bd-orchestration/skills/plan-hardening/formulas/plan-hardening-final.formula.toml.j2 the ceremony step runs after the final iteration's critical step (final formula is wisped only after the last iteration wisp closes)

Fixtures:
- pass: The final formula skills/plan-hardening/formulas/plan-hardening-final.formula.toml.j2 starts with a ceremony step, and SKILL.md wisps it only after the last iteration's critical-plan-reviewer step has closed, so ceremony-qa reviews the plan after the final critical pass (test_final_formula_starts_with_ceremony).
- fail: ceremony-qa runs in iteration 1 in parallel with the critical-plan-reviewer and is not run again after the final critical pass.

## ANC-003

Anchor: The atm-bd plan-hardening SKILL.md must tell the planning agent that a FAIL from the final critical-plan-reviewer pass is normal because that review is adversarial.

> It must be clear to planner that failures on final @.claude/agents/critical-plan-reviewer.md is normal since this is an adversarial review

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Final critical FAIL is normal

Scope: adds or changes plan-hardening SKILL.md, the fix step instructions or the step template

Deterministic check to pair with the development gate: grep -n 'normal' plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md near the final critical-plan-reviewer pass

Fixtures:
- pass: skills/plan-hardening/SKILL.md and the fix step description tell the planner: a FAIL from the final critical-plan-reviewer pass is normal because that review is adversarial; fix the findings and continue to ceremony-qa.
- fail: The final critical-plan-reviewer FAIL makes the planner start another iteration until it passes.

## ANC-004

Anchor: The atm-bd plan-hardening SKILL.md must set the default number of review iterations to 2.

> typical/default # of iterations is 2

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Default two iterations

Scope: adds or changes the plan-hardening iteration count, formulas or SKILL.md loop

Deterministic check to pair with the development gate: grep -n 'iterations' plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md shows default 2

Fixtures:
- pass: skills/plan-hardening/SKILL.md: run 2 iterations by default (the lead or planner may set another count); the loop wisps plan-hardening-iteration for iteration 1 and 2.
- fail: The skill runs reviewer cycles until convergence with a cap of 3.

## ANC-005

Anchor: In the atm-bd plan-hardening iteration formula, the plan-scope-reviewer step and the critical-plan-reviewer step must not depend on each other, so the two passes run in parallel.

> 2 iterations of @plan-scope-reviewer and @critical-plan-reviewer and adversaries (if phase-lead orchestrated) run in parallel

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Passes in an iteration run in parallel

Scope: adds or changes the plan-hardening iteration formula step dependencies or adversary dispatch

Deterministic check to pair with the development gate: the scope and critical steps in plugins/atm-bd-orchestration/skills/plan-hardening/formulas/plan-hardening-iteration.formula.toml.j2 have no needs entry naming each other

Fixtures:
- pass: In plan-hardening-iteration.formula.toml.j2 the scope and critical steps have no needs entries; only the fix step needs both, and the lead dispatches adversary tasks at the same time, so all passes of an iteration run in parallel (test_iteration_review_steps_are_parallel).
- fail: The critical step needs the scope step, so the critical-plan-reviewer starts only after plan-scope-reviewer closes.

## ANC-006

Anchor: The atm-bd `adversarial-plan-hardening` reference must forbid giving an adversary the same task in iteration 2 as in iteration 1.

> adversaries should not be given the same task both iterations. new tasks based either on weaknesses uncovered or mixing things up for complete plan coverage

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Adversaries get new tasks in iteration 2

Scope: adds or changes the adversarial-plan-hardening reference or adversary assignment

Deterministic check to pair with the development gate: grep -n 'same task' plugins/atm-bd-orchestration/skills/plan-hardening/references/adversarial-plan-hardening.md

Fixtures:
- pass: references/adversarial-plan-hardening.md: never give an adversary the same task in iteration 2; each iteration-2 task targets a weakness iteration 1 found or a plan area no one has covered yet, and the lead records each adversary's area per iteration.
- fail: The lead re-sends the same adversary tasks unchanged in iteration 2 to confirm their findings.

## ANC-007

Anchor: The atm-bd `adversarial-plan-hardening` reference must instruct the phase lead to review the plan itself and file its own findings, in addition to dispatching reviewers.

> phase-lead should work to find weaknesses in the plan, not simply coordinate other agents

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Phase lead files its own findings

Scope: adds or changes the adversarial-plan-hardening reference

Deterministic check to pair with the development gate: grep -n 'own findings' plugins/atm-bd-orchestration/skills/plan-hardening/references/adversarial-plan-hardening.md

Fixtures:
- pass: references/adversarial-plan-hardening.md: the phase lead reviews the plan itself every iteration and files its own findings as plan-fix beads, besides dispatching reviewers and adversaries.
- fail: The phase lead only dispatches reviewers and forwards their findings; it files none of its own.

## ANC-008

Anchor: The atm-bd `adversarial-plan-hardening` reference must instruct the phase lead to list every integration the plan has with another repository or an external API and to assign an agent to research each listed integration for detail the plan is missing.

> adversarial-plan-hardening would describe the process of evaluating the plan and identifying areas that may be weak i.e. integrating w/ another repo or external API in which other agents could research and identify areas not covered w/ enough detail

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Research each external repo or API integration

Scope: adds or changes the adversarial-plan-hardening reference

Deterministic check to pair with the development gate: grep -n -i 'external API' plugins/atm-bd-orchestration/skills/plan-hardening/references/adversarial-plan-hardening.md

Fixtures:
- pass: references/adversarial-plan-hardening.md step: list every integration the plan has with another repository or external API (CLI, schema, service) and assign an agent to research each one for details the plan leaves out.
- fail: The adversarial reference covers internal sprint shape only and names no research of other repositories or APIs.

## ANC-009

Anchor: The atm-bd `adversarial-plan-hardening` reference must instruct a review of each existing and new requirement and ADR the plan relates to for inconsistency, gaps and vague language, with existing ones considered for revision.

> All related Requirements/ADR both existing and new should be reviewed for consistency/gaps/vague language/opportunities to tighten ... there should also be a note that existing req/adr should be considered for revision/update

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Review and revise related REQs and ADRs

Scope: adds or changes the adversarial-plan-hardening reference

Deterministic check to pair with the development gate: grep -n -i 'ADR' plugins/atm-bd-orchestration/skills/plan-hardening/references/adversarial-plan-hardening.md

Fixtures:
- pass: references/adversarial-plan-hardening.md step: review every requirement and ADR the plan relates to, existing and new, for inconsistency, gaps, vague language and tightening; existing ones are candidates for revision through the fix loop.
- fail: Requirements and ADRs are checked only for whether their ids exist.

## ANC-010

Anchor: The atm-bd `adversarial-plan-hardening` reference must pick each research agent's tier from the fast, normal and frontier tiers used for dev dispatch, using fast agents for broad ground and frontier models for difficult code.

> fast agent(s) if there is a lot of ground to cover (potentially even using background agents). frontier models for difficult code ... referencing the same fast/normal/frontier we use for dev-sprint planning

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Researcher tier by difficulty, dev tiers

Scope: adds or changes the adversarial-plan-hardening reference

Deterministic check to pair with the development gate: grep -n -E 'fast|normal|frontier' plugins/atm-bd-orchestration/skills/plan-hardening/references/adversarial-plan-hardening.md cites the dev dispatch difficulty rule

Fixtures:
- pass: references/adversarial-plan-hardening.md assigns researchers by the dev dispatch difficulty tiers (fast, normal, frontier, per atm-bd-orchestration SKILL.md): fast agents, background allowed, for broad ground; frontier models for difficult code.
- fail: Every adversary is a frontier model regardless of the area's difficulty.

## ANC-011

Anchor: The atm-bd plan-hardening SKILL.md must say to prefer reviewers from a different model vendor (Anthropic, OpenAI, xAI) where possible, including a different vendor in iteration 2 than in iteration 1 of the same review pass.

> mixing foundries where possible is preferred even using different foundries for rounds 1 and 2

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Mix foundries, including across rounds

Scope: adds or changes plan-hardening SKILL.md reviewer selection or the adversarial-plan-hardening reference

Deterministic check to pair with the development gate: grep -n -i 'foundry' plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md

Fixtures:
- pass: skills/plan-hardening/SKILL.md reviewer selection: prefer reviewers from different foundries (Anthropic, OpenAI, xAI) where possible, and use a different foundry for a pass in iteration 2 than in iteration 1.
- fail: All reviewers in both iterations are Claude subagents.

## ANC-012

Anchor: Each review pass of the atm-bd plan-hardening skill must get its instructions from the description of a step bead created with `bd mol wisp`, not from text written into a subagent prompt.

> I would also prefer if the instructions were written as beads (ephemeral) instead of subagent prompt.

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Instructions are ephemeral beads

Scope: adds or changes plan-hardening formulas or the step template

Deterministic check to pair with the development gate: plugins/atm-bd-orchestration/skills/plan-hardening/formulas/*.formula.toml.j2 set phase = "vapor" and the dispatch template reads instructions with bd show of the step bead

Fixtures:
- pass: Each pass's instructions are the description of its step bead in the plan-hardening-iteration or plan-hardening-final formula (phase vapor), created with bd mol wisp; the dispatch template only tells the runner to bd show its step bead and follow it.
- fail: The reviewer instructions are written into the subagent prompt text by the lead.

## ANC-013

Anchor: The atm-bd plan-hardening skill must dispatch teammates and background subagents with the same template file, which takes a `runner` variable of `teammate` or `subagent`.

> exactly atm task assign w/ template to team-mate, background subagent w/ same template? ... runner = teammate | subagent so it renders properly for both?

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: One step template for both runners

Scope: adds or changes the plan-hardening step template or its dispatch

Deterministic check to pair with the development gate: one file plugins/atm-bd-orchestration/skills/plan-hardening/templates/plan-hardening-step.xml.j2; test renders it with runner=teammate and runner=subagent and refuses any other value

Fixtures:
- pass: One template, skills/plan-hardening/templates/plan-hardening-step.xml.j2, with runner = teammate or subagent; test_step_template_renders_both_runners renders both and test_step_template_refuses_other_runner checks any other value fails the strict render.
- fail: There are two templates, plan-hardening-teammate.xml.j2 and plan-hardening-subagent.xml.j2.

## ANC-014

Anchor: The atm-bd plan-hardening dispatch template must render `atm task start` for `runner=teammate` and no `atm` command for `runner=subagent`, while both renders claim and close the step bead.

> teammate needs to do task open, bd claim where subagent simply does bd claim. at close both close bead

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Teammate starts task; both claim and close bead

Scope: adds or changes the plan-hardening step template

Deterministic check to pair with the development gate: test: teammate render contains 'atm task start' and 'bd update --claim'; subagent render contains 'bd update --claim' and no 'atm '; both contain 'bd close'

Fixtures:
- pass: The teammate render of plan-hardening-step.xml.j2 runs atm task start then bd update --claim on the step bead; the subagent render runs only bd update --claim and no atm command; both end with bd close on the step bead (test_step_template_start_and_close_by_runner).
- fail: The subagent render also runs atm task start and atm task close.

## ANC-015

Anchor: For each finding a plan-hardening review files, the atm-bd plan-hardening dispatch template must create a bd bead assigned to the planning agent to fix that finding.

> I like that, findings get assigned back to planner as fix beads

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Findings become fix beads for the planner

Scope: adds or changes how plan-hardening records findings

Deterministic check to pair with the development gate: the step template's finding block runs bd create with --assignee set to the planner variable

Fixtures:
- pass: The step template's finding block runs bd create for each finding with --assignee set to the planner and label stage:plan-fix, so every finding is a fix bead for the planner.
- fail: Findings are returned only in the reviewer's fenced JSON and listed in the close report.

## ANC-016

Anchor: Files under the atm-bd package's `skills/plan-hardening/` must read and write the plan only through `bd` commands, with no markdown plan path and no `br` path.

> bd only for atm-bd

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: bd only

Scope: adds or changes plan-hardening plan reading or writing

Deterministic check to pair with the development gate: grep -rnE '\bbr |sprint-.*\.md|docs/plans/.*\.md' plugins/atm-bd-orchestration/skills/plan-hardening returns nothing outside evals fixtures

Fixtures:
- pass: skills/plan-hardening reads the plan with bd show and bd list and writes with bd create, update and close only; no markdown plan path and no br command.
- fail: The skill reads docs/plans/phase-x/sprint-*.md when present and falls back to br.

## ANC-017

Anchor: The bd-based plan-hardening skill must be shipped in the atm-bd package manifest, so the atm-bd installer installs it.

> yes, plan-hardening should be rewritten to be bd based and installed as part of atm-bd.

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Installed as part of atm-bd

Scope: adds or changes the plan-hardening skill files or the atm-bd manifest

Deterministic check to pair with the development gate: plugins/atm-bd-orchestration/manifest.yaml lists skills/plan-hardening/SKILL.md; tests/gen_manifest.py --check passes

Fixtures:
- pass: plugins/atm-bd-orchestration/manifest.yaml lists every file under skills/plan-hardening/ and the two reviewer agents; tests/gen_manifest.py --check passes, so atm-bd skill installs them.
- fail: The skill stays in atm-core's .claude/skills/plan-hardening/ and is not in the atm-bd manifest.

## ANC-018

Anchor: The atm-bd package must contain `skills/plan-hardening/evals/` with agent prompts for four eval runs: haiku as a background subagent, haiku as a teammate, luna as a background subagent and luna as a teammate.

> I would create some evals for the skill as well in skills/plan-hardening/evals/ which have some agent prompts to test against both background haiku/luna or haiku/luna team-mates

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Evals for haiku and luna, both runners

Scope: adds or changes plan-hardening evals

Deterministic check to pair with the development gate: ls plugins/atm-bd-orchestration/skills/plan-hardening/evals/ shows prompts covering haiku/luna x subagent/teammate

Fixtures:
- pass: skills/plan-hardening/evals/ has eval prompts and a fixture plan; the run matrix is haiku and luna, each as a background subagent and as a teammate (four runs).
- fail: The evals run only the subagent runner with a frontier model.

## ANC-019

Anchor: Each eval prompt in the atm-bd `skills/plan-hardening/evals/` must include post-run questions that the eval runner asks the agent after its run, about which instructions were unclear or missing.

> the evals should interrogate the agents post run to get good feedback

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Evals interrogate the agent after the run

Scope: adds or changes plan-hardening evals

Deterministic check to pair with the development gate: each plugins/atm-bd-orchestration/skills/plan-hardening/evals/*.md prompt contains a post-run questions section

Fixtures:
- pass: Each eval prompt in skills/plan-hardening/evals/ ends with post-run questions the eval runner asks the same agent after it closes: which step was unclear, where it guessed, what it skipped and why, what it needed.
- fail: Evals only check the bead state after the run; the agent is not asked anything.

## ANC-020

Anchor: The atm-bd plan-hardening SKILL.md mode-selection rule must choose the phase-lead adversarial mode whenever the team has no assigned work.

> if team is idle, I would do adversarial every time

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Idle team means adversarial mode

Scope: adds or changes plan-hardening SKILL.md mode selection

Deterministic check to pair with the development gate: grep -n -i 'idle' plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md mode selection

Fixtures:
- pass: skills/plan-hardening/SKILL.md mode selection: when the team is idle, use lead adversarial mode every time.
- fail: Mode selection always uses planner with subagents.

## ANC-021

Anchor: The atm-bd plan-hardening SKILL.md mode-selection rule must choose planner-with-teammates when the plan is hard enough to interrupt another frontier model, typically from a different model vendor.

> the planner-with-teammates is when the plan is harder and it's worth interrupting another frontier model (typically different foundry because reviews are better)

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Hard plan means planner with teammates

Scope: adds or changes plan-hardening SKILL.md mode selection

Deterministic check to pair with the development gate: grep -n -i 'planner with teammates' plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md mode selection

Fixtures:
- pass: skills/plan-hardening/SKILL.md mode selection: use planner with teammates when the plan is hard enough to interrupt another frontier model, typically from a different foundry.
- fail: Planner with teammates is used whenever any teammate is idle.

## ANC-022

Anchor: In the atm-bd plan-hardening planner-with-subagents mode, the planning agent must run every review pass with its own subagents and need nothing from the phase lead.

> if plans change mid phase, all team-mates are busy and I typically pull a frontier model off to the side to create a new sprint or two to fill the gap. having him coordinate w/ phase-lead (who is busy coordinating) isn't reasonable. it's easier for him to use subagents

Said by Rand, planning session 2026-11-03 (fenix), 2026-11-03. Status: locked. Version 2.

Summary: Planner mode needs no lead

Scope: adds or changes plan-hardening SKILL.md planner mode

Deterministic check to pair with the development gate: plugins/atm-bd-orchestration/skills/plan-hardening/SKILL.md planner-with-subagents section names no phase-lead step

Fixtures:
- pass: skills/plan-hardening/SKILL.md planner with subagents: the planner wisps and dispatches every pass itself, including the final critical-plan-reviewer and ceremony-qa passes, to its own background subagents; no step waits on the phase lead.
- fail: In planner mode the phase lead dispatches the final critical-plan-reviewer and ceremony-qa passes.

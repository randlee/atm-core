# Anchors - phase-current

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `doc/plan/phase-current/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | proposed | Installer --github/--azure select template commands | 2026-10-31 |
| ANC-002 | proposed | Lite hook: one file, organized by transition type | 2026-10-27 |
| ANC-003 | proposed | Lite transitions: exhaustive enum matches | 2026-10-27 |
| ANC-004 | proposed | Tests share repeated patterns via helpers | 2026-10-28 |
| ANC-005 | proposed | Same tool interfaces for both profiles | 2026-10-31 |
| ANC-006 | proposed | One core schema for both profiles | 2026-10-31 |
| ANC-007 | proposed | Lite leaves sanity schema fields undefined | 2026-10-31 |
| ANC-008 | proposed | No gh stack anywhere | 2026-10-31 |
| ANC-009 | proposed | Git-only base/top stack check | 2026-10-31 |
| ANC-010 | proposed | Close hook: stack gate blocks, then bd close, then task close | 2026-10-31 |
| ANC-011 | proposed | One profile per phase | 2026-10-31 |
| ANC-012 | proposed | Profile files live under the profile skill | 2026-10-31 |
| ANC-013 | proposed | Shared files installed once to .atm-bd | 2026-10-31 |
| ANC-014 | proposed | Lite has its own hook file | 2026-10-31 |
| ANC-015 | proposed | Lite runs with one agent, no sanity | 2026-10-27 |

## ANC-001

Anchor: The installer must accept --github and --azure flags that select the provider commands rendered into the sc-compose templates.

> could we add --github and --azure flags which inject the proper commands into sc-compose j2 templates?

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Installer --github/--azure select template commands

Scope: adds or changes the installer, provider vars files, sc_render or templates that run PR commands

Deterministic check to pair with the development gate: atm-bd skill --azure renders a dispatch template containing 'az repos pr' and no 'gh pr'

Fixtures:
- pass: The installer gains --github and --azure flags; the chosen provider's command set is merged into the template render context.
- fail: Templates keep hard-coded gh pr commands and the installer has no provider flag.

## ANC-002

Anchor: The lite hook's transition logic must live in one Python file organized by transition type.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-27. Status: proposed. Version 2.

Summary: Lite hook: one file, organized by transition type

Scope: adds or changes the lite hook atm-bd-task-hook.py or splits logic out of it

Deterministic check to pair with the development gate: no module under skills/atm-bd-lite/scripts other than atm-bd-task-hook.py calls bd close, bd update, atm task or git

Fixtures:
- pass: All dev, fix and qa close transitions are implemented in skills/atm-bd-lite/scripts/atm-bd-task-hook.py, one section per transition.
- fail: The lite close logic is split across hook.py, qa_close.py and dispatch.py helper modules.

## ANC-003

Anchor: Every transition in the lite hook must be an enum matched exhaustively, so that a missing arm fails pyright strict.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-27. Status: proposed. Version 2.

Summary: Lite transitions: exhaustive enum matches

Scope: adds or changes transition handling in the lite hook

Deterministic check to pair with the development gate: pyright --strict on atm-bd-task-hook.py with reportMatchNotExhaustive=error reports 0 errors

Fixtures:
- pass: Transition is a StrEnum and every handler uses match with no default arm; pyright strict reportMatchNotExhaustive is on.
- fail: Transitions are compared as strings in if/elif chains with a final else that logs unknown.

## ANC-004

Anchor: Lite hook tests must put each repeated setup or assertion pattern in a shared helper method.

> I want less code in the business logic.  test code is fine, but should be written efficiently w/ repeated patterns using common methods.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-28. Status: proposed. Version 2.

Summary: Tests share repeated patterns via helpers

Scope: adds or changes tests for the lite hook or lite cron

Deterministic check to pair with the development gate: a duplicate-block scan over skills/atm-bd-lite/scripts/tests finds no repeated block longer than 5 lines

Fixtures:
- pass: Tests build hooks through a make_close_vars() and run_hook() helper shared by every case.
- fail: Each test repeats the twelve-line close-vars setup and subprocess stub inline.

## ANC-005

Anchor: The atm-bd console, cron and queues commands must serve lite and full phases through one unchanged command-line interface.

> Some items would be atm-bd-<profile> specific i.e. atm-bd-task-hook.py the task assignment j2 templates, the cron script.  We should use the same tool interfaces for atm-bd cron, atm-bd console, atm-bd queues (and others I may have missed).  This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Same tool interfaces for both profiles

Scope: adds or changes atm-bd console, cron, queues, sprint-report or bv-analyze

Deterministic check to pair with the development gate: atm-bd console/cron/queues --help text is unchanged, and the lite fixture phase renders in each with no profile flag

Fixtures:
- pass: atm-bd console, cron and queues read profile from the phase toml and render the lite phase with the same arguments as today.
- fail: A new atm-bd lite-console command is added for lite phases.

## ANC-006

Anchor: Both profiles must use the single bead schema defined in atm-bd-core.

> Some items would be atm-bd-<profile> specific i.e. atm-bd-task-hook.py the task assignment j2 templates, the cron script.  We should use the same tool interfaces for atm-bd cron, atm-bd console, atm-bd queues (and others I may have missed).  This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: One core schema for both profiles

Scope: adds or changes bead_schema, close_fields, bead models or formulas

Deterministic check to pair with the development gate: skills/atm-bd-lite contains no schema or TypedDict model files

Fixtures:
- pass: Lite beads validate against core bead_schema models; no schema file is added under skills/atm-bd-lite.
- fail: skills/atm-bd-lite/schemas/lite-dev-bead.schema.json defines a separate lite schema.

## ANC-007

Anchor: In the lite profile the sanity fields of the shared schema must be optional, left undefined rather than filled.

> Some items would be atm-bd-<profile> specific i.e. atm-bd-task-hook.py the task assignment j2 templates, the cron script.  We should use the same tool interfaces for atm-bd cron, atm-bd console, atm-bd queues (and others I may have missed).  This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Lite leaves sanity schema fields undefined

Scope: adds or changes bead_schema, close_fields or close vars

Deterministic check to pair with the development gate: core bead_schema and CloseFields mark sanity_bead and max_sanity_rounds NotRequired, and a lite close-vars fixture without them validates

Fixtures:
- pass: sanity_bead and max_sanity_rounds are NotRequired in CloseFields and lite close vars omit them.
- fail: Lite close vars set sanity_bead to the string none to satisfy the required field.

## ANC-008

Anchor: Scripts and templates of either profile never invoke the gh stack subcommand or the GitHub stacks API.

> One item that also needs to be addressed both ATM – BD – lite and ATM – BD – orchestration is for GH stacking to work on non-GitHub repos, a.k.a. azure git repos.  This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: No gh stack anywhere

Scope: adds or changes stack linking, stack checks, sanity-stack-link, or dispatch templates

Deterministic check to pair with the development gate: grep -rn 'gh stack\|/stacks' over core/ and skills/ (*.py, *.j2, sanity-stack-link) finds nothing

Fixtures:
- pass: sanity-stack-link records layers in the stack bead and sets the PR base with the provider command; no gh stack call remains.
- fail: The lite hook calls gh stack link to attach the layer.

## ANC-009

Anchor: Stack checks must compare the PR base with the stack top using git alone.

> One item that also needs to be addressed both ATM – BD – lite and ATM – BD – orchestration is for GH stacking to work on non-GitHub repos, a.k.a. azure git repos.  This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Git-only base/top stack check

Scope: adds or changes the stack gate or stack linking

Deterministic check to pair with the development gate: stack_gate.py subprocess calls are git only; a test with an Azure-style remote URL passes the gate

Fixtures:
- pass: stack_gate.check runs git merge-base --is-ancestor on origin/<base> and head and compares the base ref to the stack top branch.
- fail: The stack check queries the GitHub stacks API for the stack's top PR.

## ANC-010

Anchor: The dev and fix close hook must refuse the close when the PR base differs from the stack top, before running bd close and atm task close.

> 1) dev/fix 'atm task close' would check pr base vs. stack top (blocking close if not stacked properly) then do bd close, atm task close.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Close hook: stack gate blocks, then bd close, then task close

Scope: adds or changes a dev or fix close hook or the stack gate

Deterministic check to pair with the development gate: a hook test with PR base != stack top exits non-zero with no bd or atm subprocess recorded

Fixtures:
- pass: On NotStacked the hook exits 2 with the fix command before any bd close or atm task close subprocess.
- fail: The hook runs bd close first and then warns that the PR base does not match the stack top.

## ANC-011

Anchor: A phase never contains beads of more than one profile.

> 2) never mix profiles in the same phase.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: One profile per phase

Scope: adds or changes phase-config, validate-plan, bead-groups or bead_schema

Deterministic check to pair with the development gate: validate-plan on a lite phase containing a stage:dev-sanity bead exits 5

Fixtures:
- pass: validate-plan exits 5 when a lite phase contains a stage:dev-sanity bead.
- fail: A phase mixes lite sprints and full sprints by setting profile per sprint bead.

## ANC-012

Anchor: Profile-specific scripts and templates must live under their own skill directory, skills/<profile>/scripts and skills/<profile>/templates.

> i would expect the atm-bd-lite only scripts to be in skills/atm-bd-lite/scripts/  and templates in atm-bd-lite/templates/ w/ corresponding atm-bd-orchestration scripts/stemplates located relative to the skill.  scripts/templates would be at a common point (which perhaps would best be at repo root in .atm-bd/scripts and /templates.  That would remove duplicates across .claude/ .codex/ and .agents/ that exist today

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Profile files live under the profile skill

Scope: adds or moves scripts or templates of a profile

Deterministic check to pair with the development gate: the full profile's hook, sanity scripts and dev/fix/sanity templates sit under skills/atm-bd-orchestration; the lite hook, cron and templates sit under skills/atm-bd-lite

Fixtures:
- pass: atm-bd-task-hook.py, lite_cron.py and the lite dev/fix/qa templates sit under skills/atm-bd-lite/scripts and templates.
- fail: The lite hook is placed in core/scripts next to the shared modules.

## ANC-013

Anchor: Shared scripts and templates must be installed once, to .atm-bd/scripts and .atm-bd/templates at the repository root.

> i would expect the atm-bd-lite only scripts to be in skills/atm-bd-lite/scripts/  and templates in atm-bd-lite/templates/ w/ corresponding atm-bd-orchestration scripts/stemplates located relative to the skill.  scripts/templates would be at a common point (which perhaps would best be at repo root in .atm-bd/scripts and /templates.  That would remove duplicates across .claude/ .codex/ and .agents/ that exist today

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Shared files installed once to .atm-bd

Scope: adds, moves or installs shared scripts or templates

Deterministic check to pair with the development gate: after install, no file under .atm-bd/scripts or .atm-bd/templates also exists under .claude/skills, .codex/skills or .agents

Fixtures:
- pass: core/scripts and core/templates install to .atm-bd/scripts and .atm-bd/templates; the skill directories no longer carry copies.
- fail: Shared modules are copied into .claude/skills and .codex/skills on install as before.

## ANC-014

Anchor: The atm-bd-lite profile must ship its own task-close hook file, separate from the full profile's atm-task-close.py.

> the atm-bd-lite would have a completely different atm-task-hook python file.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Lite has its own hook file

Scope: adds or changes a task-close hook, or selects a hook by profile

Deterministic check to pair with the development gate: test -f plugins/atm-bd-orchestration/skills/atm-bd-lite/scripts/atm-bd-task-hook.py; grep -c lite plugins/atm-bd-orchestration/skills/atm-bd-orchestration/scripts/atm-task-close.py prints 0

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-hook.py is a separate file; atm-task-close.py is unchanged.
- fail: atm-task-close.py gains an if profile == lite branch that skips sanity.

## ANC-015

Anchor: The lite profile must run a phase end to end with a single agent and no sanity stage.

> we haven't designed 'lite' yet. it will be a much simpler orchestration that could be run by a single agent or small team.

Said by Rand, planning session 2026-10-31 (fenix), 2026-10-27. Status: proposed. Version 2.

Summary: Lite runs with one agent, no sanity

Scope: adds or changes the lite skill, lite formulas or lite templates

Deterministic check to pair with the development gate: the lite sprint-group formula pours dev and qa beads only, and the lite SKILL.md names no sanity member

Fixtures:
- pass: The lite sprint-group formula pours dev and qa beads; one agent dispatches, develops and closes QA in the live run.
- fail: The lite formula pours a sanity bead that a second agent must close before QA.

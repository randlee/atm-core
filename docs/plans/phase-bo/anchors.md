# Anchors - phase-bo

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bo/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | proposed | Installer --github/--azure select template commands | 2026-10-31 |
| ANC-002 | proposed | Lite hook business logic in one file | 2026-10-31 |
| ANC-003 | proposed | Transitions are enums with exhaustive checks | 2026-10-31 |
| ANC-004 | proposed | Repeated test patterns use common helpers | 2026-10-31 |
| ANC-005 | proposed | Same tool interfaces for every profile | 2026-10-31 |
| ANC-006 | proposed | One bead schema for all profiles | 2026-10-31 |
| ANC-007 | proposed | Lite leaves sanity schema items undefined | 2026-10-31 |
| ANC-008 | proposed | Stack check is git-only, no gh stack | 2026-10-31 |
| ANC-009 | proposed | Stack check compares base and top only | 2026-10-31 |
| ANC-010 | proposed | Close hook blocks on PR base vs stack top mismatch | 2026-10-31 |
| ANC-011 | proposed | One profile per phase | 2026-10-31 |
| ANC-012 | proposed | Profile-only files under the profile skill | 2026-10-31 |
| ANC-013 | proposed | Shared files installed once to .atm-bd | 2026-10-31 |
| ANC-014 | proposed | Lite has its own task hook file | 2026-10-31 |
| ANC-015 | proposed | Lite runs with one agent | 2026-10-31 |
| ANC-016 | proposed | Result values, no throwing | 2026-10-31 |
| ANC-017 | proposed | Fundamental types in one Rust crate | 2026-10-31 |
| ANC-018 | proposed | Python fixes release without the crate | 2026-10-31 |
| ANC-019 | proposed | Hook organized by transition type | 2026-10-31 |
| ANC-020 | proposed | Shared types live in atm-bd-core | 2026-10-31 |
| ANC-021 | proposed | Stacking works on Azure Git repos | 2026-10-31 |
| ANC-022 | proposed | pyright covers every plugin and console script | 2026-10-31 |
| ANC-023 | proposed | Profile comes from the phase file; hook, templates, cron and console align on it | 2026-10-31 |
| ANC-024 | proposed | No console on a lite phase | 2026-10-31 |
| ANC-025 | proposed | GitHub prompts use gh stack; gh stack view --json is the view | 2026-10-31 |

## ANC-001

Anchor: The installer must accept --github and --azure flags that select the provider commands rendered into the sc-compose templates.

> could we add --github and --azure flags which inject the proper commands into sc-compose j2 templates?

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Installer --github/--azure select template commands

Scope: adds or changes the installer, provider vars files, sc_render or templates that run PR commands

Deterministic check to pair with the development gate: atm-bd skill --azure renders a dispatch template containing 'az repos pr' and no 'gh pr'

Fixtures:
- pass: The installer gains --github and --azure flags (install.py, test_install_provider_flags) and the chosen provider's command set is merged into the template render context; see the PR diff of install.py.
- fail: Templates keep hard-coded gh pr commands and the installer has no provider flag.

## ANC-002

Anchor: The lite profile's task-close hook business logic must live in a single Python file, skills/atm-bd-lite/scripts/atm-bd-task-hook.py.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Lite hook business logic in one file

Scope: adds or changes the lite hook or moves hook logic into another module

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts contains one hook file and no other module imports or defines transition handling

Fixtures:
- pass: All dev, fix and qa close transitions are implemented in skills/atm-bd-lite/scripts/atm-bd-task-hook.py, one section per transition.
- fail: The lite close logic is split across hook.py, qa_close.py and dispatch.py helper modules.

## ANC-003

Anchor: Transitions in the lite hook must be enum members with exhaustive checks, so that an unhandled member is a static error.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Transitions are enums with exhaustive checks

Scope: adds or changes transition handling in the lite hook

Deterministic check to pair with the development gate: pyright strict with reportMatchNotExhaustive=error passes on the hook and a test removing one match arm makes it fail

Fixtures:
- pass: Transition is a StrEnum and every handler uses match with no default arm; pyright strict with reportMatchNotExhaustive=error passes on the hook (pyright output in the PR, test_transition_exhaustive).
- fail: Transitions are compared as strings in if/elif chains with a final else that logs unknown.

## ANC-004

Anchor: Test code written in the phase must implement its repeated patterns through common helper methods instead of repeating them inline.

> test code is fine, but should be written efficiently w/ repeated patterns using common methods.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Repeated test patterns use common helpers

Scope: adds or changes Python tests in atm-bd-orchestration

Deterministic check to pair with the development gate: no setup or assertion block of five or more identical lines appears in two test functions of the touched test files

Fixtures:
- pass: Tests build hooks through the shared make_close_vars() and run_hook() helpers in tests/lite/conftest.py; no test repeats that setup inline (diff of tests/lite/test_hook.py).
- fail: Each test repeats the twelve-line close-vars setup and subprocess stub inline.

## ANC-005

Anchor: atm-bd cron, atm-bd console and atm-bd queues must be invoked with the same subcommands and arguments for a lite phase as for a full phase.

> We should use the same tool interfaces for atm-bd cron, atm-bd console, atm-bd queues (and others I may have missed).

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 5.

Summary: Same tool interfaces for every profile

Scope: adds or changes atm-bd console, cron, queues, sprint-report or bv-analyze

Deterministic check to pair with the development gate: the --help output of atm-bd console, cron and queues is byte-identical before and after the phase

Fixtures:
- pass: atm-bd console, cron and queues keep their argv and read the profile from the phase toml; on a lite phase queues prints the lite columns, cron runs the lite tick and console exits with the typed ProfileMismatch error, all through the same subcommands and arguments as today; their --help output is byte-identical before and after (tests/test_cli_help_unchanged.py).
- fail: A new atm-bd lite-console command is added for lite phases.

## ANC-006

Anchor: All profiles must use the same schema.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: One bead schema for all profiles

Scope: adds or changes bead_schema, close_fields, bead models or formulas

Deterministic check to pair with the development gate: no schema file exists under skills/atm-bd-lite and lite beads validate against the core bead_schema models

Fixtures:
- pass: Lite beads validate against the core bead_schema models; no schema file is added under skills/atm-bd-lite.
- fail: skills/atm-bd-lite/schemas/lite-dev-bead.schema.json defines a separate lite schema.

## ANC-007

Anchor: The shared schema must allow the lite profile to leave its sanity items undefined.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Lite leaves sanity schema items undefined

Scope: adds or changes bead_schema, close_fields or close vars

Deterministic check to pair with the development gate: a lite close-vars file without any sanity key validates, and a full close-vars file without them is rejected

Fixtures:
- pass: sanity_bead and max_sanity_rounds are NotRequired in CloseFields (core/scripts/close_fields.py) and a lite close-vars file without them validates (test_lite_close_vars_without_sanity).
- fail: Lite close vars set sanity_bead to the string none to satisfy the required field.

## ANC-008

Anchor: The stack check of the dev and fix close hook never invokes the gh stack subcommand or the GitHub stacks API.

> This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 5.

Summary: Stack check is git-only, no gh stack

Scope: adds or changes the stack gate called by the dev or fix close hook

Deterministic check to pair with the development gate: grep -rn 'gh stack' plugins/atm-bd-orchestration/skills plugins/atm-bd-orchestration/core finds only prose

Fixtures:
- pass: stack_gate.py compares the PR base ref with the stack top through git only; test_stack_gate_no_gh asserts no gh subprocess is spawned during check, and the hook test shows the gate result produced with the gh binary absent from PATH.
- fail: The close hook's stack check calls gh stack view --json to find the layer below.

## ANC-009

Anchor: The stack check must verify stacking by comparing only the PR base and the stack top branch, using no other stack record.

> This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Stack check compares base and top only

Scope: adds or changes the stack gate or stack linking

Deterministic check to pair with the development gate: stack_gate.py calls git merge-base and reads the PR base ref and no other stack source

Fixtures:
- pass: The stack check compares the PR base ref with the stack top branch and reads no other stack record; stack_gate.py makes no gh or API call (test_stack_gate_git_only, PR diff of stack_gate.py).
- fail: The stack check queries the GitHub stacks API for the stack's top PR.

## ANC-010

Anchor: The dev and fix close hook must compare the PR base with the stack top (the branch of the layer below) and block the close when they differ, before it runs bd close and atm task close.

> 1) dev/fix 'atm task close' would check pr base vs. stack top (blocking close if not stacked properly) then do bd close, atm task close.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Close hook blocks on PR base vs stack top mismatch

Scope: adds or changes a dev or fix close hook or the stack gate

Deterministic check to pair with the development gate: a hook test with a mis-based PR exits non-zero and records no bd close or atm task close command

Fixtures:
- pass: The dev close hook compares the PR base with the stack top first and exits 2 when they differ; bd close and atm task close run only after that check passes (test_hook_blocks_misbased_pr records no bd or atm command).
- fail: The hook runs bd close first and then warns that the PR base does not match the stack top.

## ANC-011

Anchor: All beads of a phase must carry the same profile value (lite or full) as the phase file.

> 2) never mix profiles in the same phase.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: One profile per phase

Scope: adds or changes phase-config, validate-plan, bead-groups or bead_schema

Deterministic check to pair with the development gate: validate-plan exits non-zero on a phase file whose beads carry two profiles

Fixtures:
- pass: validate-plan rejects a lite phase that contains a stage:dev-sanity bead with exit 5 (test_validate_plan_mixed_profiles; run log in the PR).
- fail: A phase mixes lite sprints and full sprints by setting the profile per sprint bead.

## ANC-012

Anchor: Lite-only scripts must live in skills/atm-bd-lite/scripts and lite-only templates in skills/atm-bd-lite/templates; full-only ones live relative to the atm-bd-orchestration skill.

> i would expect the atm-bd-lite only scripts to be in skills/atm-bd-lite/scripts/  and templates in atm-bd-lite/templates/ w/ corresponding atm-bd-orchestration scripts/stemplates located relative to the skill.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Profile-only files under the profile skill

Scope: adds or moves scripts or templates of a profile

Deterministic check to pair with the development gate: every file under skills/atm-bd-lite/scripts and templates is imported or rendered only by lite; core/ holds nothing lite-only

Fixtures:
- pass: atm-bd-task-hook.py, lite_cron.py and the lite dev/fix/qa templates sit under skills/atm-bd-lite/scripts and skills/atm-bd-lite/templates.
- fail: The lite hook is placed in core/scripts next to the shared modules.

## ANC-013

Anchor: Shared scripts and templates must live at one common point, .atm-bd/scripts and .atm-bd/templates at the repository root, removing the duplicates under .claude, .codex and .agents.

> scripts/templates would be at a common point (which perhaps would best be at repo root in .atm-bd/scripts and /templates.  That would remove duplicates across .claude/ .codex/ and .agents/ that exist today

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Shared files installed once to .atm-bd

Scope: adds, moves or installs shared scripts or templates

Deterministic check to pair with the development gate: after atm-bd skill install, each shared module exists once under .atm-bd and diff finds no copy under .claude, .codex or .agents

Fixtures:
- pass: core/scripts and core/templates install to .atm-bd/scripts and .atm-bd/templates; the skill directories no longer carry copies.
- fail: Shared modules are copied into .claude/skills and .codex/skills on install as before.

## ANC-014

Anchor: The atm-bd-lite skill must have its own task-hook Python file, separate from the atm-bd-orchestration skill's atm-task-close.py.

> the atm-bd-lite would have a completely different atm-task-hook python file.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Lite has its own task hook file

Scope: adds or changes a task-close hook, or selects a hook by profile

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts contains a hook file and the full profile's atm-task-close.py has no lite branch

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-hook.py is a separate file; atm-task-close.py is unchanged.
- fail: atm-task-close.py gains an if profile == lite branch that skips sanity.

## ANC-015

Anchor: The lite profile's orchestration must be runnable by a single agent.

> it will be a much simpler orchestration that could be run by a single agent or small team.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Lite runs with one agent

Scope: adds or changes the lite skill, lite formulas or lite templates

Deterministic check to pair with the development gate: the bo-7 live lite phase log shows one ATM identity performing dispatch, dev, QA and landing

Fixtures:
- pass: The live lite phase log in bo-7 shows one ATM identity performing dispatch, dev, QA and landing; the lite formula pours only dev and qa beads (atm-bd queues output attached).
- fail: The lite formula pours a sanity bead that a second agent must close before QA can start.

## ANC-016

Anchor: New or rewritten Python code in the phase must return Result<T,Err> values instead of throwing exceptions.

> use pyright, return Result<T,Err> (no throwing)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Result values, no throwing

Scope: adds or rewrites Python code in atm-bd-orchestration

Deterministic check to pair with the development gate: grep raise in the files created by the phase finds only programmer-error guards; every public function returns Ok or Err

Fixtures:
- pass: profile.py returns Result[Profile]; main maps Err to exit code 2 and JSON output.
- fail: load_provider raises ValueError on an unknown provider and callers catch it.

## ANC-017

Anchor: The fundamental atm-bd types must be defined in one Rust crate.

> I am fine w/ you building a rust crate w/ the fundamental types if it can easily be distributed.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 2.

Summary: Fundamental types in one Rust crate

Scope: adds or changes shared enums, error types, provider or stack-gate types, or their distribution

Deterministic check to pair with the development gate: crates/atm-bd-types defines Profile, Provider, Transition and Error and no Python module redefines them

Fixtures:
- pass: The Profile, Provider and Transition enums and the Error type are defined in crates/atm-bd-types and exposed to Python through its wheel.
- fail: The shared enums are defined in Python dataclasses under core/scripts and no Rust crate exists.

## ANC-018

Anchor: A quick Python-only fix of atm-bd must release without a crate publish step.

> I am hopeful we won't be changing the atm-bd crate too often so we don't need a complicated publish process for quick python fixes

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Python fixes release without the crate

Scope: adds or changes release workflows, version files or the crate pin

Deterministic check to pair with the development gate: release.yml for a Python-only tag runs no crate publish job

Fixtures:
- pass: atm-bd 0.21.1 shipped a Python-only fix: release.yml ran no crate publish job and atm-bd-types stays at 0.2.0 (workflow run log and the crates.io version page).
- fail: Every Python release bumps and republishes atm-bd-types at the same version number.

## ANC-019

Anchor: The lite task-hook file must be organized by transition type: one handler section per transition (dev close, fix close, QA close).

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Hook organized by transition type

Scope: adds or changes the lite hook

Deterministic check to pair with the development gate: the hook file has one handler per Transition member and no handler serves two transitions

Fixtures:
- pass: atm-bd-task-hook.py has one handler section each for dev close, fix close and qa close, selected by the Transition enum (file outline in the PR; test_one_handler_per_transition).
- fail: The hook is organized by bead field, with one function per close-vars key that branches on the transition inside.

## ANC-020

Anchor: Types shared between all profiles must be defined in atm-bd-core.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 3.

Summary: Shared types live in atm-bd-core

Scope: adds or changes enums, dataclasses, models or schema types used by a profile or a shared tool

Deterministic check to pair with the development gate: no module under skills/atm-bd-lite or skills/atm-bd-orchestration defines a type imported by another skill or by core

Fixtures:
- pass: Profile, Provider and Transition are defined once in atm-bd-core (crates/atm-bd-types, re-exported by core/scripts) and imported by both profile hooks; grep finds no second definition (PR diff).
- fail: skills/atm-bd-lite/scripts/types.py defines its own Transition enum copied from the full profile.

## ANC-021

Anchor: Stack linking and the stack check in both profiles must work on an Azure Git repository, not only on GitHub.

> One item that also needs to be addressed both ATM – BD – lite and ATM – BD – orchestration is for GH stacking to work on non-GitHub repos, a.k.a. azure git repos.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 4.

Summary: Stacking works on Azure Git repos

Scope: adds or changes stack linking, the stack gate, or provider commands

Deterministic check to pair with the development gate: a stack gate test against an Azure-style remote URL passes and the azure render of the dispatch templates contains az repos pr commands

Fixtures:
- pass: Stack linking and the stack check both pass against an Azure-style remote: test_stack_gate_azure_remote maps an Azure Git URL to a local bare repository with git insteadOf, and test_stack_link_azure links a layer on it with az repos pr commands (test log in the PR).
- fail: The stack check requires the gh CLI and a GitHub remote; Azure repositories are out of scope.

## ANC-022

Anchor: Every Python file and extensionless Python script in the plugin and the console must be inside the pyright include and pass pyright in just lint and CI.

> use pyright, return Result<T,Err> (no throwing)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: proposed. Version 6.

Summary: pyright covers every plugin and console script

Scope: adds, moves or changes any Python file or script in plugins/, apps/atm-bd-console or .github/scripts, or pyrightconfig.json

Deterministic check to pair with the development gate: a test walks the tree for *.py files and python-shebang scripts and asserts each is matched by the pyrightconfig include; just lint runs pyright at 0 errors

Fixtures:
- pass: pyrightconfig.json include covers plugins/, apps/atm-bd-console, release_scripts, .github/scripts and every extensionless script including bead-queues; tests/test_pyright_include_complete.py walks the tree and asserts each Python file and python-shebang script is matched; just lint runs pyright with 0 errors (CI log for the head).
- fail: Deliverable: pyrightconfig.json lists only the moved core modules; bead-queues and the .github/scripts files stay outside the include with no test over the include, and just lint runs pyright on the console package alone.

## ANC-023

Anchor: The task hook, the dispatch templates, the cron script and the console must each read the profile (lite or full) from the phase file and refuse to run when the phase file's profile is not their own.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note, 2026-10-31. Status: proposed. Version 2.

Summary: Profile comes from the phase file; hook, templates, cron and console align on it

Scope: adds or changes the task hook, the dispatch templates, the cron script, the console, or the phase file schema

Deterministic check to pair with the development gate: every entrypoint of the hook, cron script and console calls the shared phase-file profile reader, and a test asserts the mismatch exit for each of the four

Fixtures:
- pass: atm-bd-task-hook.py, the dispatch templates, atm-bd cron and atm-bd console each call the shared reader atm_bd_core.profile.read_phase_profile(phase_file) and exit with PROFILE.MISMATCH when the phase file's profile is not theirs; tests/test_profile_gate.py asserts the mismatch exit for all four.
- fail: The console takes the profile from a --profile command-line flag that defaults to full and never reads the phase file.

## ANC-024

Anchor: The console never dispatches work for a phase whose phase file sets the lite profile.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note, 2026-10-31. Status: proposed. Version 2.

Summary: No console on a lite phase

Scope: adds or changes atm-bd console or its dispatch path

Deterministic check to pair with the development gate: a test runs atm-bd console against a phase file with profile lite and asserts a refusal exit with no dispatch

Fixtures:
- pass: atm-bd console --phase bo exits with PROFILE.MISMATCH and dispatches nothing when .atm-bd/phase-bo.toml sets profile = "lite"; tests/test_console_refuses_lite.py asserts the exit code and an empty dispatch log.
- fail: atm-bd console dispatches the ready beads of a lite phase because the profile check was added only to the cron script.

## ANC-025

Anchor: Agent prompt templates rendered for the GitHub provider must use gh stack commands for stack work, with gh stack view --json as the stack view.

> we can and should use ‘gh stack’ in agent prompts if github is used.  ‘gh stack view —json’ is best view of stack and inexpensive if tokens available.

Said by Rand, planning session 2026-10-31 (fenix), note on ANC-008, 2026-10-31. Status: proposed. Version 2.

Summary: GitHub prompts use gh stack; gh stack view --json is the view

Scope: adds or changes dispatch templates or the provider command vars files

Deterministic check to pair with the development gate: the github provider vars file maps the stack_view op to gh stack view --json and stack_link to gh stack link; a render test asserts both strings in the github render and neither in the azure render

Fixtures:
- pass: providers/github.json maps stack_link to gh stack link and stack_view to gh stack view --json; the dev template rendered with --github contains both and rendered with --azure contains neither (test_provider_render_stack_ops, render diffs in the PR body).
- fail: The dev template rendered for GitHub tells the agent to inspect the stack with one gh pr view per branch and never mentions gh stack.

# Anchors - phase-bo

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bo/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Installer --github/--azure select template commands | 2026-10-31 |
| ANC-002 | retired | Lite hook business logic in one file | 2026-10-31 |
| ANC-003 | locked | Transitions are enums with exhaustive checks | 2026-10-31 |
| ANC-004 | locked | Repeated test patterns use common helpers | 2026-10-31 |
| ANC-005 | locked | Same tool interfaces for every profile | 2026-10-31 |
| ANC-006 | locked | One bead schema for all profiles | 2026-10-31 |
| ANC-007 | retired | Lite leaves sanity schema items undefined | 2026-10-31 |
| ANC-008 | retired | Stack check is git-only, no gh stack | 2026-10-31 |
| ANC-009 | retired | Stack check compares base and top only | 2026-10-31 |
| ANC-010 | retired | Close hook blocks on PR base vs stack top mismatch | 2026-10-31 |
| ANC-011 | retired | One profile per phase | 2026-10-31 |
| ANC-012 | locked | Profile-only files under the profile skill | 2026-10-31 |
| ANC-013 | retired | Shared files installed once to .atm-bd | 2026-10-31 |
| ANC-014 | locked | Lite has its own task hook file | 2026-10-31 |
| ANC-015 | locked | Lite runs with one agent | 2026-10-31 |
| ANC-016 | locked | Result values, no throwing | 2026-10-31 |
| ANC-017 | locked | Fundamental types in one Rust crate | 2026-10-31 |
| ANC-018 | locked | Python fixes release without the crate | 2026-10-31 |
| ANC-019 | locked | Hook organized by transition type | 2026-10-31 |
| ANC-020 | locked | Shared types live in atm-bd-core | 2026-10-31 |
| ANC-021 | retired | Stacking works on Azure Git repos | 2026-10-31 |
| ANC-022 | retired | pyright covers every plugin and console script | 2026-10-31 |
| ANC-023 | retired | Profile comes from the phase file; hook, templates, cron and console align on it | 2026-10-31 |
| ANC-024 | locked | No console on a lite phase | 2026-10-31 |
| ANC-025 | retired | GitHub prompts use gh stack; gh stack view --json is the view | 2026-10-31 |
| ANC-026 | locked | Lite hook and templates refuse a non-lite phase file | 2026-10-31 |
| ANC-027 | locked | Cron ticks refuse the other profile's phase file | 2026-10-31 |
| ANC-028 | locked | atm-bd-lite is its own skill directory | 2026-11-01 |
| ANC-029 | retired | Shared code is in the shared location | 2026-11-01 |
| ANC-030 | locked | atm-bd-orchestration is not changed to use shared code | 2026-11-01 |
| ANC-031 | locked | Profile hooks, templates and cron live in the profile's skill | 2026-11-01 |
| ANC-032 | locked | atm-bd commands call the profile's same-named script | 2026-11-01 |
| ANC-033 | retired | Each console data source is its own Rust trait | 2026-11-01 |
| ANC-034 | locked | Every trait ships a generated .pyi stub | 2026-11-01 |
| ANC-035 | retired | atm-bd-lite shares no files with atm-bd-orchestration | 2026-11-01 |
| ANC-036 | locked | Shared code is newly written | 2026-11-01 |
| ANC-037 | retired | Console data sources share no code | 2026-11-01 |
| ANC-038 | locked | Python trait implementations pass pyright strict | 2026-11-01 |
| ANC-039 | locked | Lite work does not break atm-bd-orchestration | 2026-11-01 |
| ANC-040 | locked | Non-lite-specific code goes in the shared location | 2026-11-01 |
| ANC-041 | locked | Shared code is tested on orchestration-profile inputs | 2026-11-01 |
| ANC-042 | locked | Shared code is tested on lite-profile inputs | 2026-11-01 |
| ANC-043 | retired | Shared code matches orchestration's output | 2026-11-01 |
| ANC-044 | locked | Shared code has no profile-specific logic | 2026-11-01 |
| ANC-045 | locked | Lite uses shared code in the sprint that adds it | 2026-11-01 |
| ANC-046 | retired | Shared module design lists orchestration requirements | 2026-11-01 |
| ANC-047 | retired | Shared interfaces support every listed orchestration behavior | 2026-11-01 |
| ANC-048 | locked | Lite task.assign hook is its own file | 2026-11-01 |
| ANC-049 | locked | Lite task.start hook is its own file | 2026-11-01 |
| ANC-050 | locked | Lite task.close hook is its own file | 2026-11-01 |
| ANC-051 | locked | Each lite hook has its own state-transition diagram | 2026-11-01 |
| ANC-052 | locked | One diagram row per hook match arm | 2026-11-01 |
| ANC-053 | locked | Jev: every diagram row is implemented | 2026-11-01 |
| ANC-054 | locked | Jev: every code arm is in the diagram | 2026-11-01 |
| ANC-055 | locked | Lite has no close-vars or state file | 2026-11-01 |
| ANC-056 | locked | Lite hooks look up facts at hook time | 2026-11-01 |
| ANC-057 | locked | Lite dev close is one atm task close | 2026-11-01 |
| ANC-058 | locked | Missing fact means refuse with the fix | 2026-11-01 |
| ANC-059 | locked | Lite has no repair or backfill scripts | 2026-11-01 |
| ANC-060 | locked | Lite never calls wrapper-blocked gh commands | 2026-11-01 |
| ANC-061 | locked | No profile names hard-coded in atm-bd-types | 2026-11-01 |
| ANC-062 | locked | No dicts or Any in lite code; use atm-bd-types | 2026-11-01 |
| ANC-063 | locked | lite beads access only via beads-cache | 2026-11-01 |
| ANC-064 | proposed | one beads-cache write per hook | 2026-11-01 |
| ANC-065 | proposed | hooks only read the ledger | 2026-11-01 |
| ANC-066 | proposed | no subprocess in a hook | 2026-11-01 |
| ANC-067 | proposed | stack and pour are template blocks | 2026-11-01 |
| ANC-068 | proposed | stub carries only used names | 2026-11-01 |
| ANC-069 | proposed | stack bead is the atomic ledger | 2026-11-01 |
| ANC-070 | proposed | hook file <= 100 lines | 2026-11-01 |
| ANC-071 | proposed | only Rand commits the spec | 2026-11-01 |
| ANC-072 | proposed | review findings bind to the spec | 2026-11-01 |
| ANC-073 | proposed | anchor checks are mechanical | 2026-11-01 |

## ANC-001

Anchor: The installer must accept --github and --azure flags that select the provider commands rendered into the sc-compose templates.

> could we add --github and --azure flags which inject the proper commands into sc-compose j2 templates?

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Installer --github/--azure select template commands

Scope: adds or changes the installer, provider vars files, sc_render or templates that run PR commands

Deterministic check to pair with the development gate: atm-bd skill --azure renders a dispatch template containing 'az repos pr' and no 'gh pr'

Fixtures:
- pass: The installer gains --github and --azure flags (install.py, test_install_provider_flags) and the chosen provider's command set is merged into the template render context; see the PR diff of install.py.
- fail: Templates keep hard-coded gh pr commands and the installer has no provider flag.

## ANC-002

Anchor: The lite profile's task-close hook business logic must live in a single Python file, skills/atm-bd-lite/scripts/atm-bd-task-hook.py.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 4.

Summary: Lite hook business logic in one file

Scope: adds or changes the lite hook or moves hook logic into another module

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts contains one hook file and no other module imports or defines transition handling

Fixtures:
- pass: All dev, fix and qa close transitions are implemented in skills/atm-bd-lite/scripts/atm-bd-task-hook.py, one section per transition.
- fail: The lite close logic is split across hook.py, qa_close.py and dispatch.py helper modules.

## ANC-003

Anchor: Transitions in the lite hook must be enum members with exhaustive checks, so that an unhandled member is a static error.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Transitions are enums with exhaustive checks

Scope: adds or changes transition handling in the lite hook

Deterministic check to pair with the development gate: pyright strict with reportMatchNotExhaustive=error passes on the hook and a test removing one match arm makes it fail

Fixtures:
- pass: Transition is a StrEnum and every handler uses match with no default arm; pyright strict with reportMatchNotExhaustive=error passes on the hook (pyright output in the PR, test_transition_exhaustive).
- fail: Transitions are compared as strings in if/elif chains with a final else that logs unknown.

## ANC-004

Anchor: Test code written in the phase must implement its repeated patterns through common helper methods instead of repeating them inline.

> test code is fine, but should be written efficiently w/ repeated patterns using common methods.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 5.

Summary: Repeated test patterns use common helpers

Scope: adds or changes Python tests in atm-bd-orchestration

Deterministic check to pair with the development gate: no setup or assertion block of five or more identical lines appears in two test functions of the touched test files

Fixtures:
- pass: Hook tests build the request and the in-memory cache through shared make_request() and FakeCache helpers in tests/lite/conftest.py; no test repeats that setup inline (diff of tests/lite/test_task_close_hook.py).
- fail: Each hook test repeats the twelve-line request and fake-cache setup inline.

## ANC-005

Anchor: atm-bd cron, atm-bd console and atm-bd queues must be invoked with the same subcommands and arguments for a lite phase as for a orchestration phase.

> We should use the same tool interfaces for atm-bd cron, atm-bd console, atm-bd queues (and others I may have missed).

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 7.

Summary: Same tool interfaces for every profile

Scope: adds or changes atm-bd console, cron, queues, sprint-report or bv-analyze

Deterministic check to pair with the development gate: the --help output of atm-bd console, cron and queues is byte-identical before and after the phase

Fixtures:
- pass: atm-bd console, cron and queues keep their argv and read the profile from the phase toml; on a lite phase queues prints the lite columns, cron runs the lite tick and console exits with the typed ProfileMismatch error, all through the same subcommands and arguments as today; their --help output is byte-identical before and after (tests/test_cli_help_unchanged.py).
- fail: A new atm-bd lite-console command is added for lite phases.

## ANC-006

Anchor: All profiles must use the same schema.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 3.

Summary: One bead schema for all profiles

Scope: adds or changes bead_schema, close_fields, bead models or formulas

Deterministic check to pair with the development gate: no schema file exists under skills/atm-bd-lite and lite beads validate against the core bead_schema models

Fixtures:
- pass: Lite beads validate against the core bead_schema models; no schema file is added under skills/atm-bd-lite.
- fail: skills/atm-bd-lite/schemas/lite-dev-bead.schema.json defines a separate lite schema.

## ANC-007

Anchor: The shared schema must allow the lite profile to leave its sanity items undefined.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 6.

Summary: Lite leaves sanity schema items undefined

Scope: adds or changes bead_schema, close_fields or close vars

Deterministic check to pair with the development gate: a lite close-vars file without any sanity key validates, and a orchestration close-vars file without them is rejected

Fixtures:
- pass: sanity_bead and max_sanity_rounds are NotRequired in CloseFields (core/scripts/close_fields.py) and a lite close-vars file without them validates (test_lite_close_vars_without_sanity).
- fail: Lite close vars set sanity_bead to the string none to satisfy the required field.

## ANC-008

Anchor: The stack check of the dev and fix close hook never invokes the gh stack subcommand or the GitHub stacks API.

> This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 7.

Summary: Stack check is git-only, no gh stack

Scope: adds or changes the stack gate called by the dev or fix close hook

Deterministic check to pair with the development gate: grep -rn 'gh stack' plugins/atm-bd-orchestration/skills plugins/atm-bd-orchestration/core finds only prose

Fixtures:
- pass: stack_gate.py compares the PR base ref with the stack top through git only; test_stack_gate_no_gh asserts no gh subprocess is spawned during check, and the hook test shows the gate result produced with the gh binary absent from PATH.
- fail: The close hook's stack check calls gh stack view --json to find the layer below.

## ANC-009

Anchor: The stack check must verify stacking by comparing only the PR base and the stack top branch, using no other stack record.

> This is something we should address at ATM BD lite phase buy only checking the base and top of work trees, not using the GH stack sub command

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 5.

Summary: Stack check compares base and top only

Scope: adds or changes the stack gate or stack linking

Deterministic check to pair with the development gate: stack_gate.py calls git merge-base and reads the PR base ref and no other stack source

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bo-2`: bo-2 only declares the StackGateResult type and the StackGate Protocol; the PR-base-versus-stack-top comparison is implemented and judged in bo-4, which satisfies ANC-009.

Fixtures:
- pass: The stack check compares the PR base ref with the stack top branch and reads no other stack record; stack_gate.py makes no gh or API call (test_stack_gate_git_only, PR diff of stack_gate.py).
- fail: The stack check queries the GitHub stacks API for the stack's top PR.

## ANC-010

Anchor: The dev and fix close hook must compare the PR base with the stack top (the branch of the layer below) and block the close when they differ, before it runs bd close and atm task close.

> 1) dev/fix 'atm task close' would check pr base vs. stack top (blocking close if not stacked properly) then do bd close, atm task close.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 5.

Summary: Close hook blocks on PR base vs stack top mismatch

Scope: adds or changes a dev or fix close hook or the stack gate

Deterministic check to pair with the development gate: a hook test with a mis-based PR exits non-zero and records no bd close or atm task close command

Fixtures:
- pass: The dev close hook compares the PR base with the stack top first and exits 2 when they differ; bd close and atm task close run only after that check passes (test_hook_blocks_misbased_pr records no bd or atm command).
- fail: The hook runs bd close first and then warns that the PR base does not match the stack top.

## ANC-011

Anchor: All beads of a phase must carry the same profile value (lite or orchestration) as the phase file.

> 2) never mix profiles in the same phase.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 6.

Summary: One profile per phase

Scope: adds or changes phase-config, validate-plan, bead-groups or bead_schema

Deterministic check to pair with the development gate: validate-plan exits non-zero on a phase file whose beads carry two profiles

Fixtures:
- pass: validate-plan rejects a lite phase that contains a stage:dev-sanity bead with exit 5 (test_validate_plan_mixed_profiles; run log in the PR).
- fail: A phase mixes lite sprints and orchestration sprints by setting the profile per sprint bead.

## ANC-012

Anchor: Lite-only scripts must live in skills/atm-bd-lite/scripts and lite-only templates in skills/atm-bd-lite/templates; orchestration-only ones live relative to the atm-bd-orchestration skill.

> i would expect the atm-bd-lite only scripts to be in skills/atm-bd-lite/scripts/  and templates in atm-bd-lite/templates/ w/ corresponding atm-bd-orchestration scripts/stemplates located relative to the skill.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 5.

Summary: Profile-only files under the profile skill

Scope: adds or moves scripts or templates of a profile

Deterministic check to pair with the development gate: every file under skills/atm-bd-lite/scripts and templates is imported or rendered only by lite; core/ holds nothing lite-only

Fixtures:
- pass: The three lite task hooks and the lite dev/fix/qa templates sit under skills/atm-bd-lite/scripts and skills/atm-bd-lite/templates.
- fail: The lite hook is placed in core/scripts next to the shared modules.

## ANC-013

Anchor: Shared scripts and templates must live at one common point, .atm-bd/scripts and .atm-bd/templates at the repository root, removing the duplicates under .claude, .codex and .agents.

> scripts/templates would be at a common point (which perhaps would best be at repo root in .atm-bd/scripts and /templates.  That would remove duplicates across .claude/ .codex/ and .agents/ that exist today

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 5.

Summary: Shared files installed once to .atm-bd

Scope: adds, moves or installs shared scripts or templates

Deterministic check to pair with the development gate: after atm-bd skill install, each shared module exists once under .atm-bd and diff finds no copy under .claude, .codex or .agents

Fixtures:
- pass: core/scripts and core/templates install to .atm-bd/scripts and .atm-bd/templates; the skill directories no longer carry copies.
- fail: Shared modules are copied into .claude/skills and .codex/skills on install as before.

## ANC-014

Anchor: The atm-bd-lite skill must have its own task-hook Python file, separate from the atm-bd-orchestration skill's atm-task-close.py.

> the atm-bd-lite would have a completely different atm-task-hook python file.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Lite has its own task hook file

Scope: adds or changes a task-close hook, or selects a hook by profile

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts contains a hook file and the orchestration profile's atm-task-close.py has no lite branch

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-hook.py is a separate file; atm-task-close.py is unchanged.
- fail: atm-task-close.py gains an if profile == lite branch that skips sanity.

## ANC-015

Anchor: The lite profile's orchestration must be runnable by a single agent.

> it will be a much simpler orchestration that could be run by a single agent or small team.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Lite runs with one agent

Scope: adds or changes the lite skill, lite formulas or lite templates

Deterministic check to pair with the development gate: the bo-7 live lite phase log shows one ATM identity performing dispatch, dev, QA and landing

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bo-6`: bo-6 changes read-only views and the console refusal; single-agent runnability of the lite loop is delivered by bo-5 and proved live by bo-7, both satisfying ANC-015.

Fixtures:
- pass: The live lite phase log in bo-7 shows one ATM identity performing dispatch, dev, QA and landing; the lite formula pours only dev and qa beads (atm-bd queues output attached).
- fail: The lite formula pours a sanity bead that a second agent must close before QA can start.

## ANC-016

Anchor: New or rewritten Python code in the phase must return Result<T,Err> values instead of throwing exceptions.

> use pyright, return Result<T,Err> (no throwing)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 3.

Summary: Result values, no throwing

Scope: adds or rewrites Python code in atm-bd-orchestration

Deterministic check to pair with the development gate: grep raise in the files created by the phase finds only programmer-error guards; every public function returns Ok or Err

Fixtures:
- pass: profile.py returns Result[Profile]; main maps Err to exit code 2 and JSON output.
- fail: load_provider raises ValueError on an unknown provider and callers catch it.

## ANC-017

Anchor: The fundamental atm-bd types must be defined in one Rust crate.

> I am fine w/ you building a rust crate w/ the fundamental types if it can easily be distributed.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Fundamental types in one Rust crate

Scope: adds or changes shared enums, error types, provider or stack-gate types, or their distribution

Deterministic check to pair with the development gate: crates/atm-bd-types defines Transition and ErrorId (no Profile type, ANC-061) and grep finds no class Transition or class ErrorId under skills/atm-bd-lite

Fixtures:
- pass: The Transition and ErrorId enums and the Refusal type are defined in crates/atm-bd-types and exposed to Python through its wheel; the profile stays a name read from the phase file.
- fail: The shared enums are defined in Python dataclasses under core/scripts and no Rust crate exists.

## ANC-018

Anchor: A quick Python-only fix of atm-bd must release without a crate publish step.

> I am hopeful we won't be changing the atm-bd crate too often so we don't need a complicated publish process for quick python fixes

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Python fixes release without the crate

Scope: adds or changes release workflows, version files or the crate pin

Deterministic check to pair with the development gate: release.yml for a Python-only tag runs no crate publish job

Fixtures:
- pass: atm-bd 0.21.1 shipped a Python-only fix: release.yml ran no crate publish job and atm-bd-types stays at 0.2.0 (workflow run log and the crates.io version page).
- fail: Every Python release bumps and republishes atm-bd-types at the same version number.

## ANC-019

Anchor: The lite task-hook file must be organized by transition type: one handler section per transition (dev close, fix close, QA close).

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 4.

Summary: Hook organized by transition type

Scope: adds or changes the lite hook

Deterministic check to pair with the development gate: the hook file has one handler per Transition member and no handler serves two transitions

Fixtures:
- pass: atm-bd-task-hook.py has one handler section each for dev close, fix close and qa close, selected by the Transition enum (file outline in the PR; test_one_handler_per_transition).
- fail: The hook is organized by bead field, with one function per close-vars key that branches on the transition inside.

## ANC-020

Anchor: Types shared between all profiles must be defined in atm-bd-core.

> This would mean that there would need to be atm-bd-core types that are shared between all profiles and I expect we would want the same schema, although atm-bd-lite would allow some mandetory schema items to be 'undefined' i.e. sanity.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: locked. Version 7.

Summary: Shared types live in atm-bd-core

Scope: adds or changes enums, dataclasses, models or schema types used by a profile or a shared tool

Deterministic check to pair with the development gate: no module under skills/atm-bd-lite or skills/atm-bd-orchestration defines a type imported by another skill or by core

Fixtures:
- pass: Transition and ErrorId are defined once in crates/atm-bd-types and imported by the three lite hooks; grep finds no second definition (PR diff).
- fail: skills/atm-bd-lite/scripts/types.py defines its own Transition enum copied from the orchestration profile.

## ANC-021

Anchor: Stack linking and the stack check in both profiles must work on an Azure Git repository, not only on GitHub.

> One item that also needs to be addressed both ATM – BD – lite and ATM – BD – orchestration is for GH stacking to work on non-GitHub repos, a.k.a. azure git repos.

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 6.

Summary: Stacking works on Azure Git repos

Scope: adds or changes stack linking, the stack gate, or provider commands

Deterministic check to pair with the development gate: a stack gate test against an Azure-style remote URL passes and the azure render of the dispatch templates contains az repos pr commands

Fixtures:
- pass: Stack linking and the stack check both pass against an Azure-style remote: test_stack_gate_azure_remote maps an Azure Git URL to a local bare repository with git insteadOf, and test_stack_link_azure links a layer on it with az repos pr commands (test log in the PR).
- fail: The stack check requires the gh CLI and a GitHub remote; Azure repositories are out of scope.

## ANC-022

Anchor: Every Python file and extensionless Python script in the plugin and the console must be inside the pyright include and pass pyright in just lint and CI.

> use pyright, return Result<T,Err> (no throwing)

Said by Rand, planning session 2026-10-27..31 (fenix), 2026-10-31. Status: retired. Version 10.

Summary: pyright covers every plugin and console script

Scope: adds, moves or changes any Python file or script in plugins/, apps/atm-bd-console or .github/scripts, or pyrightconfig.json

Deterministic check to pair with the development gate: a test walks the tree for *.py files and python-shebang scripts and asserts each is matched by the pyrightconfig include; just lint runs pyright at 0 errors

Fixtures:
- pass: pyrightconfig.json include covers plugins/, apps/atm-bd-console, release_scripts, .github/scripts and every python-shebang extensionless script; tests/test_pyright_include_complete.py walks the tree and asserts each Python file and python-shebang script is matched while shell wrappers such as bead-queues are not; just lint runs pyright with 0 errors (CI log for the head).
- fail: Deliverable: pyrightconfig.json lists only the moved core modules; the .github/scripts files stay outside the include with no test over the include, and just lint runs pyright on the console package alone.

## ANC-023

Anchor: The task hook, the dispatch templates, the cron script and the console must each read the profile (lite or full) from the phase file and refuse to run when the phase file's profile is not their own.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note, 2026-10-31. Status: retired. Version 4.

Summary: Profile comes from the phase file; hook, templates, cron and console align on it

Scope: adds or changes the task hook, the dispatch templates, the cron script, the console, or the phase file schema

Deterministic check to pair with the development gate: every entrypoint of the hook, cron script and console calls the shared phase-file profile reader, and a test asserts the mismatch exit for each of the four

Fixtures:
- pass: atm-bd-task-hook.py, the dispatch templates, atm-bd cron and atm-bd console each call the shared reader atm_bd_core.profile.read_phase_profile(phase_file) and exit with PROFILE.MISMATCH when the phase file's profile is not theirs; tests/test_profile_gate.py asserts the mismatch exit for all four.
- fail: The console takes the profile from a --profile command-line flag that defaults to full and never reads the phase file.

## ANC-024

Anchor: The console never dispatches work for a phase whose phase file sets the lite profile.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note, 2026-10-31. Status: locked. Version 3.

Summary: No console on a lite phase

Scope: adds or changes atm-bd console or its dispatch path

Deterministic check to pair with the development gate: a test runs atm-bd console against a phase file with profile lite and asserts a refusal exit with no dispatch

Fixtures:
- pass: atm-bd console --phase bo exits with PROFILE.MISMATCH and dispatches nothing when .atm-bd/phase-bo.toml sets profile = "lite"; tests/test_console_refuses_lite.py asserts the exit code and an empty dispatch log.
- fail: atm-bd console dispatches the ready beads of a lite phase because the profile check was added only to the cron script.

## ANC-025

Anchor: Agent prompt templates rendered for the GitHub provider must use gh stack commands for stack work, with gh stack view --json as the stack view.

> we can and should use ‘gh stack’ in agent prompts if github is used.  ‘gh stack view —json’ is best view of stack and inexpensive if tokens available.

Said by Rand, planning session 2026-10-31 (fenix), note on ANC-008, 2026-10-31. Status: retired. Version 4.

Summary: GitHub prompts use gh stack; gh stack view --json is the view

Scope: adds or changes dispatch templates or the provider command vars files

Deterministic check to pair with the development gate: the github provider vars file maps the stack_view op to gh stack view --json and stack_link to gh stack link; a render test asserts both strings in the github render and neither in the azure render

Fixtures:
- pass: providers/github.json maps stack_link to gh stack link and stack_view to gh stack view --json; the dev template rendered with --github contains both and rendered with --azure contains neither (test_provider_render_stack_ops, render diffs in the PR body).
- fail: The dev template rendered for GitHub tells the agent to inspect the stack with one gh pr view per branch and never mentions gh stack.

## ANC-026

Anchor: The lite task hook and the lite dispatch templates must take the profile from the phase file and refuse to run or render when it is not lite.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note; split of ANC-023 under Rand goal "complete phase-bo plan with qa + anc all passing and start Bo sprints", 2026-10-31. Status: locked. Version 3.

Summary: Lite hook and templates refuse a non-lite phase file

Scope: adds or changes the lite task hook, its runner, or the lite dispatch templates

Deterministic check to pair with the development gate: a test runs the hook runner and renders each lite template against a orchestration phase file and asserts the ProfileMismatch exit

Fixtures:
- pass: hook_runner.py and the lite template render call require_profile(phase_file, "lite") first and exit with ProfileMismatch on a orchestration phase file; tests/test_lite_profile_gate.py asserts the exit for the runner and for each lite template.
- fail: The lite hook takes the profile from a --profile argument and runs its close plan against a orchestration phase file without reading the phase file.

## ANC-027

Anchor: Each atm-bd cron tick must take the profile from the phase file and refuse to run when it is not the tick's own profile.

> the lite/orchestration flag is set in phase toml/json file.  You can’t run console for lite if agents are orchestrating.  This is critically important.  hooks/templates/cron/console must all align.

Said by Rand, planning session 2026-10-31 (fenix), ANC-005 note; split of ANC-023 under Rand goal "complete phase-bo plan with qa + anc all passing and start Bo sprints", 2026-10-31. Status: locked. Version 4.

Summary: Cron ticks refuse the other profile's phase file

Scope: adds or changes atm-bd cron or a profile cron tick

Deterministic check to pair with the development gate: a test runs the orchestration tick on a lite phase file and the lite tick on a orchestration phase file and asserts the ProfileMismatch exit for both

Fixtures:
- pass: The lite cron script, which has the same file name as orchestration's (ANC-032), reads the profile from the phase file and exits with ProfileMismatch on an orchestration phase file. A test runs each profile's tick on the other profile's phase file and asserts ProfileMismatch.
- fail: The lite cron script runs its tick on whatever phase file it is given and lists queues for an orchestration phase.

## ANC-028

Anchor: atm-bd-lite must be its own skill in the skills/atm-bd-lite/ directory.

> Lite is separate.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: locked. Version 4.

Summary: atm-bd-lite is its own skill directory

Scope: adds or moves files under skills/atm-bd-lite/ or skills/atm-bd-orchestration/

Deterministic check to pair with the development gate: skills/atm-bd-lite/SKILL.md exists and names the skill atm-bd-lite

Fixtures:
- pass: The lite profile ships as plugins/atm-bd-orchestration/skills/atm-bd-lite/ with its own SKILL.md (name: atm-bd-lite); test_lite_skill_layout asserts the directory and skill name.
- fail: Lite is a mode flag inside skills/atm-bd-orchestration/SKILL.md; no skills/atm-bd-lite directory exists.

## ANC-029

Anchor: Code used by more than one profile must be placed in the shared location.

> The shared code is new code, placed in the shared location.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: retired. Version 3.

Summary: Shared code is in the shared location

Scope: adds code to the shared location or to atm-bd-types

Deterministic check to pair with the development gate: no module under a profile skill is imported by another profile's skill

## ANC-030

Anchor: atm-bd-orchestration skill files must not be changed to use the shared code.

> atm-bd-orchestration doesn't change to use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: locked. Version 4.

Summary: atm-bd-orchestration is not changed to use shared code

Scope: touches any file under skills/atm-bd-orchestration/

Deterministic check to pair with the development gate: git diff --name-only <phase base>..<phase top> -- plugins/atm-bd-orchestration/skills/atm-bd-orchestration/ is empty

Fixtures:
- pass: git diff origin/main..HEAD -- plugins/atm-bd-orchestration/skills/atm-bd-orchestration/ is empty; orchestration keeps importing its own scripts and none of them imports the shared location.
- fail: skills/atm-bd-orchestration/scripts/atm-task-close.py now imports stack_gate from the shared location instead of its own stack_top module.

## ANC-031

Anchor: Each profile's task hooks, task-assignment templates and cron script must be inside that profile's own skill directory.

> The profile-specific parts (the hooks, the templates and the cron script) live in each profile's own skill

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: locked. Version 4.

Summary: Profile hooks, templates and cron live in the profile's skill

Scope: adds or moves a task hook, a task-assignment template or a cron script

Deterministic check to pair with the development gate: find shows every lite hook, template and cron script under skills/atm-bd-lite/; none in the shared location

Fixtures:
- pass: The three lite task hooks and the lite dev/fix/qa assignment templates are all under skills/atm-bd-lite/ (scripts/ and templates/); find shows none of them in the shared location.
- fail: The lite dev-template.xml.j2 is placed in the shared location next to the shared modules.

## ANC-032

Anchor: Each atm-bd subcommand must run the script with the same file name from the skill of the profile set in the phase file.

> and the atm-bd commands just call the profile's scripts by the same name.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: locked. Version 5.

Summary: atm-bd commands call the profile's same-named script

Scope: adds or changes how an atm-bd subcommand selects or calls profile code

Deterministic check to pair with the development gate: a test runs each subcommand on a lite and a orchestration phase file and asserts the called script path is <profile skill>/scripts/<same file name>

Fixtures:
- pass: atm-bd queues reads profile=lite from .atm-bd/phase-x.toml and runs skills/atm-bd-lite/scripts/queues; test_subcommand_routing asserts the called path for lite and orchestration phase files for console, cron, queues, show and snapshot.
- fail: atm-bd cron runs a new lite-cron entry point with a different file name and argument list for lite phases.

## ANC-033

Anchor: Each console data source (sanity-log, qa-log, pr-log and any later source) must be its own Rust trait in atm-bd-types.

> console data must have separate traits per source.  i.e. separate from the work today, I want things like sanity-log, qa-log, pr-log to be absolutely independent code. | a protocol can not be consumed by rust.  a trait can be consumed by both.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: retired. Version 5.

Summary: Each console data source is its own Rust trait

Scope: adds or changes an atm-bd console data source

Deterministic check to pair with the development gate: atm-bd-types defines one trait per console data source

Fixtures:
- pass: atm-bd-types defines trait SanityLogSource, trait QaLogSource and trait PrLogSource in src/console_sources.rs, one trait per console data source.
- fail: All console data sources implement one generic DataSource trait with a source-kind enum argument.

## ANC-034

Anchor: Every atm-bd-types trait must ship a .pyi stub generated from the trait.

> approved ` .pyi stub `

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand approved the wording ("yes on anchors"); Jev rubric not run: JEV.HTTP 402, 2026-11-01. Status: locked. Version 4.

Summary: Every trait ships a generated .pyi stub

Scope: adds or changes an atm-bd-types trait or a Python implementation of one

Deterministic check to pair with the development gate: regenerating the stubs produces no diff

Fixtures:
- pass: crates/atm-bd-types/atm_bd_types.pyi is generated by stub_gen from the traits; CI regenerates it and fails on any diff.
- fail: The .pyi stub is hand-written and has drifted from the Rust trait signatures.

## ANC-035

Anchor: atm-bd-lite must share no files with the atm-bd-orchestration skill.

> Lite is separate.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); split from a compound anchor at Rand direction "separate anchors for single requirement per anchor" (from ANC-028), 2026-11-01. Status: retired. Version 2.

Summary: atm-bd-lite shares no files with atm-bd-orchestration

Scope: adds or moves files under skills/atm-bd-lite/ or skills/atm-bd-orchestration/

Deterministic check to pair with the development gate: no path under skills/atm-bd-lite/ is a symlink into skills/atm-bd-orchestration/, and grep of skills/atm-bd-lite for atm-bd-orchestration paths is empty

## ANC-036

Anchor: Code in the shared location must be newly written, not moved or copied out of atm-bd-orchestration.

> The shared code is new code, placed in the shared location.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); split from a compound anchor at Rand direction "separate anchors for single requirement per anchor" (from ANC-029), 2026-11-01. Status: locked. Version 3.

Summary: Shared code is newly written

Scope: adds code to the shared location or to atm-bd-types

Deterministic check to pair with the development gate: git log --follow on every shared-location file shows no rename or copy from a skills/atm-bd-orchestration/ path

Fixtures:
- pass: The shared phase_file.py module is new code written for this phase; git log --follow shows it created in sprint bo-10 with no rename or copy from skills/atm-bd-orchestration/.
- fail: shared/stack_top.py is skills/atm-bd-orchestration/scripts/stack_top.py moved with git mv and lightly edited.

## ANC-037

Anchor: Console data source implementations must share no code with each other apart from the atm-bd-types shared types.

> console data must have separate traits per source.  i.e. separate from the work today, I want things like sanity-log, qa-log, pr-log to be absolutely independent code.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); split from a compound anchor at Rand direction "separate anchors for single requirement per anchor" (from ANC-033), 2026-11-01. Status: retired. Version 4.

Summary: Console data sources share no code

Scope: adds or changes an atm-bd console data source

Deterministic check to pair with the development gate: each source module imports no other source module; each source has its own fixture-input test

Fixtures:
- pass: shared/sources/qa_log.py and shared/sources/pr_log.py import only atm_bd_types and the standard library; neither imports the other.
- fail: pr_log.py imports the line parser helper from qa_log.py.

## ANC-038

Anchor: Every Python implementation of an atm-bd-types trait must pass pyright strict against the trait's .pyi stub.

> approved ` .pyi stub `

Said by Rand, phase-bo replan session 2026-11-01 (fenix); split from a compound anchor at Rand direction "separate anchors for single requirement per anchor" (from ANC-034), 2026-11-01. Status: locked. Version 3.

Summary: Python trait implementations pass pyright strict

Scope: adds or changes a Python implementation of an atm-bd-types trait

Deterministic check to pair with the development gate: pyright --strict on the Python implementations passes

Fixtures:
- pass: The Python QaLogSource implementation passes pyright --strict against atm_bd_types.pyi in just lint and CI.
- fail: The Python implementation is excluded from the pyright include and uses Any for the trait's return types.

## ANC-039

Anchor: No change made for atm-bd-lite may break atm-bd-orchestration.

> Lite code is separate (i.e. no breaking changed to orchestration), lite code is divided into lite specific and shared code.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand clarified "Lite is separate"; follow-up: "shared code is not used by atm-bd-orchestration yet, it would be a breaking change to refactor today", 2026-11-01. Status: locked. Version 4.

Summary: Lite work does not break atm-bd-orchestration

Scope: touches any file under skills/atm-bd-orchestration/, atm-bd-types or the shared location

Deterministic check to pair with the development gate: the atm-bd-orchestration test suite passes at the phase top, and git diff <phase base>..<phase top> shows no atm-bd-orchestration test file changed

Fixtures:
- pass: At the stack top the atm-bd-orchestration test suite (just test-orchestration at origin/main) passes with no orchestration test file changed.
- fail: Lite work renames an orchestration helper and edits three orchestration tests to match the new name.

## ANC-040

Anchor: atm-bd-lite code that is not specific to the lite profile must be placed in the shared location, not in skills/atm-bd-lite/.

> Lite code is separate (i.e. no breaking changed to orchestration), lite code is divided into lite specific and shared code.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand clarified "Lite is separate"; follow-up: "shared code is not used by atm-bd-orchestration yet, it would be a breaking change to refactor today", 2026-11-01. Status: locked. Version 4.

Summary: Non-lite-specific code goes in the shared location

Scope: adds code to the shared location, atm-bd-types or a profile skill

Deterministic check to pair with the development gate: no module under a profile skill is imported by another profile's skill

Fixtures:
- pass: shared/phase_file.py (profile reading, no lite-only logic) is in the shared location; skills/atm-bd-lite/ holds only the lite hooks, templates and cron.
- fail: A profile-neutral phase-file reader is written inside skills/atm-bd-lite/scripts/.

## ANC-041

Anchor: Every public function and trait implementation in the shared location must have a test that runs it on orchestration-profile inputs: a real orchestration-profile phase file and bead data from a orchestration phase.

> How the fuck do we write code that lands in a folder called 'shared' and is designed w/ shared requirements w/out actually sharing it. | If you don't do this, you will not have shared code that works WHEN we cut over and use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed the four-anchor proposal for proving shared code works for the full profile before cutover, 2026-11-01. Status: locked. Version 5.

Summary: Shared code is tested on orchestration-profile inputs

Scope: adds or changes code in the shared location or atm-bd-types

Deterministic check to pair with the development gate: test inventory: every public item in the shared location is exercised by a test whose fixture is a orchestration-profile phase file and orchestration-phase bead data

Fixtures:
- pass: tests/shared/test_phase_file.py runs read_profile on .atm-bd/phase-bj.toml and the phase-bj bead export, both real orchestration-profile data.
- fail: The shared module's tests use only a synthetic lite phase file.

## ANC-042

Anchor: Every public function and trait implementation in the shared location must have a test that runs it on lite-profile inputs.

> How the fuck do we write code that lands in a folder called 'shared' and is designed w/ shared requirements w/out actually sharing it. | If you don't do this, you will not have shared code that works WHEN we cut over and use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed the four-anchor proposal for proving shared code works for the full profile before cutover, 2026-11-01. Status: locked. Version 4.

Summary: Shared code is tested on lite-profile inputs

Scope: adds or changes code in the shared location or atm-bd-types

Deterministic check to pair with the development gate: test inventory: every public item in the shared location is exercised by a test whose fixture is a lite-profile phase file

Fixtures:
- pass: tests/shared/test_phase_file.py also runs read_profile on a lite-profile phase file fixture.
- fail: The shared stack gate is tested only against orchestration-profile phase data.

## ANC-043

Anchor: Where atm-bd-orchestration already implements the same behavior, a test must show the shared code gives the same output as the orchestration implementation on the same orchestration-profile inputs.

> How the fuck do we write code that lands in a folder called 'shared' and is designed w/ shared requirements w/out actually sharing it. | If you don't do this, you will not have shared code that works WHEN we cut over and use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed the four-anchor proposal for proving shared code works for the full profile before cutover, 2026-11-01. Status: retired. Version 5.

Summary: Shared code matches orchestration's output

Scope: adds or changes code in the shared location or atm-bd-types

Deterministic check to pair with the development gate: each shared item with an orchestration counterpart has a comparison test that calls the unmodified orchestration code and the shared code on the same orchestration-profile input and asserts equal output

Fixtures:
- pass: test_phase_file_parity calls the unmodified orchestration phase-config reader and shared read_phase on the same orchestration-profile file and asserts equal output.
- fail: The shared reader re-implements the orchestration behaviour and no test compares the two outputs.

## ANC-044

Anchor: Code in the shared location must not branch on, or hard-code, either profile.

> How the fuck do we write code that lands in a folder called 'shared' and is designed w/ shared requirements w/out actually sharing it. | If you don't do this, you will not have shared code that works WHEN we cut over and use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed the four-anchor proposal for proving shared code works for the full profile before cutover, 2026-11-01. Status: locked. Version 4.

Summary: Shared code has no profile-specific logic

Scope: adds or changes code in the shared location or atm-bd-types

Deterministic check to pair with the development gate: grep of the shared location for profile literals (lite, orchestration, "lite", "orchestration") and branches on the profile is empty

Fixtures:
- pass: grep of the shared location for "lite" and "orchestration" is empty; behaviour differences come from the caller's arguments.
- fail: shared/stack_gate.py contains "if profile == "lite": skip sanity check".

## ANC-045

Anchor: atm-bd-lite must call each shared-location module in the same sprint that adds the module.

> I don't think you understand the concept of writing 'shared' modules without cutting over and using them immediately.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed "items 1-4 are correct" for: (1) shared/ is new code, lite uses it immediately, orchestration stays untouched and cuts over later; (2) shared modules are designed now to orchestration requirements as well as lite, from an inventory of what orchestration code does today; (3) proven by orchestration-profile tests and output parity with unchanged orchestration code; (4) anchors come from that requirement set, 2026-11-01. Status: locked. Version 3.

Summary: Lite uses shared code in the sprint that adds it

Scope: adds or changes code, design or an interface in the shared location or atm-bd-types

Deterministic check to pair with the development gate: every module added to the shared location in a sprint's diff is imported by atm-bd-lite code in that same diff

Fixtures:
- pass: Sprint bo-10 adds shared/phase_file.py and skills/atm-bd-lite/scripts/atm-bd-task-hook.py imports it in the same diff.
- fail: shared/provider_commands.py lands in bo-9 and no lite code calls it until bo-12.

## ANC-046

Anchor: Each shared-location module's design must list the atm-bd-orchestration behaviors it must meet, each citing the orchestration file and function that implements it today.

> How the fuck do we write code that lands in a folder called 'shared' and is designed w/ shared requirements w/out actually sharing it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed "items 1-4 are correct" for: (1) shared/ is new code, lite uses it immediately, orchestration stays untouched and cuts over later; (2) shared modules are designed now to orchestration requirements as well as lite, from an inventory of what orchestration code does today; (3) proven by orchestration-profile tests and output parity with unchanged orchestration code; (4) anchors come from that requirement set, 2026-11-01. Status: retired. Version 4.

Summary: Shared module design lists orchestration requirements

Scope: adds or changes code, design or an interface in the shared location or atm-bd-types

Deterministic check to pair with the development gate: each shared module has a requirements table in the phase design docs; every cited orchestration path and function exists at the phase base

Fixtures:
- pass: The design section for shared/stack_gate.py lists the orchestration behaviours it must meet, each citing skills/atm-bd-orchestration/scripts/stack_top.py and the function implementing it today.
- fail: The shared stack gate design describes only what lite needs and names no orchestration file or function.

## ANC-047

Anchor: Each shared-location interface (Rust trait or .pyi stub) must support every atm-bd-orchestration behavior listed in that module's requirements.

> If you don't do this, you will not have shared code that works WHEN we cut over and use it.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); Rand confirmed "items 1-4 are correct" for: (1) shared/ is new code, lite uses it immediately, orchestration stays untouched and cuts over later; (2) shared modules are designed now to orchestration requirements as well as lite, from an inventory of what orchestration code does today; (3) proven by orchestration-profile tests and output parity with unchanged orchestration code; (4) anchors come from that requirement set, 2026-11-01. Status: retired. Version 4.

Summary: Shared interfaces support every listed orchestration behavior

Scope: adds or changes code, design or an interface in the shared location or atm-bd-types

Deterministic check to pair with the development gate: every row of each shared module's orchestration requirements table maps to an ANC-043 output-parity test

Fixtures:
- pass: Each row of the stack gate's orchestration requirements table has a same-output test; the StackGate trait signature covers all rows including the not-pushed case.
- fail: The StackGate trait omits the not-pushed result that orchestration's stack check returns today.

## ANC-048

Anchor: The lite task.assign hook must be its own Python file under skills/atm-bd-lite/scripts/.

> I want all three to be independent files and independently tested by jev against the state transition diagrams.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Lite task.assign hook is its own file

Scope: adds, moves or changes a lite task hook file

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts/ holds exactly one file registered as the task.assign hook, and it imports no other hook file

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-assign-hook.py is the lite task.assign hook and imports no other hook file.
- fail: task.assign handling is a branch inside the lite task.close hook file.

## ANC-049

Anchor: The lite task.start hook must be its own Python file under skills/atm-bd-lite/scripts/.

> I want all three to be independent files and independently tested by jev against the state transition diagrams.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Lite task.start hook is its own file

Scope: adds, moves or changes a lite task hook file

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts/ holds exactly one file registered as the task.start hook, and it imports no other hook file

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-start-hook.py is the lite task.start hook and imports no other hook file.
- fail: task.start and task.assign share one lite hook file.

## ANC-050

Anchor: The lite task.close hook must be its own Python file under skills/atm-bd-lite/scripts/.

> I want all three to be independent files and independently tested by jev against the state transition diagrams.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Lite task.close hook is its own file

Scope: adds, moves or changes a lite task hook file

Deterministic check to pair with the development gate: skills/atm-bd-lite/scripts/ holds exactly one file registered as the task.close hook, and it imports no other hook file

Fixtures:
- pass: skills/atm-bd-lite/scripts/atm-bd-task-hook.py is the lite task.close hook and imports no other hook file.
- fail: The lite task.close logic is split between atm-bd-task-hook.py and a qa_close.py helper module.

## ANC-051

Anchor: Each lite task hook file must have its own state-transition diagram in the phase design docs.

> I want all three to be independent files and independently tested by jev against the state transition diagrams.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Each lite hook has its own state-transition diagram

Scope: adds or changes a lite task hook file or its state-transition diagram

Deterministic check to pair with the development gate: the lite hooks design doc has one diagram section per hook file, named after the hook

Fixtures:
- pass: docs/plans/phase-bo/design/lite-hooks.md has one state-transition diagram section per lite hook file: task.assign, task.start, task.close.
- fail: One combined diagram covers all three lite hooks.

## ANC-052

Anchor: A lite hook's state-transition diagram must have one Transitions-table row per match arm in that hook's code.

> I want all three to be independent files and independently tested by jev against the state transition diagrams.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: One diagram row per hook match arm

Scope: adds or changes a lite task hook file or its state-transition diagram

Deterministic check to pair with the development gate: count of Transitions-table rows equals count of match arms in the hook file

Fixtures:
- pass: The task.close diagram has six Transitions rows (T-CLOSE-01..06) and the hook's match statement has six arms, one per row.
- fail: The diagram has four rows while the hook's match has six arms.

## ANC-053

Anchor: Jev must judge every Transitions-table row of a lite hook's diagram as implemented by that hook's code, with no row failing.

> I one one file for atm-bd-lite task close and jev verifies 100% matches the design.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Jev: every diagram row is implemented

Scope: adds or changes a lite task hook file or its state-transition diagram

Deterministic check to pair with the development gate: the per-hook Jev verification run reports pass for every diagram row; the run JSON is committed under anchor-runs/

Fixtures:
- pass: The Jev run anchor-runs/hooks/task-close.json judges T-CLOSE-01..06 each implemented by atm-bd-task-hook.py, no row failing.
- fail: Jev reports T-CLOSE-04 (QA FAIL pours findings) as not implemented in the hook code.

## ANC-054

Anchor: Jev must judge every match arm in a lite hook's code as listed in that hook's state-transition diagram, with no arm unlisted.

> I one one file for atm-bd-lite task close and jev verifies 100% matches the design.

Said by Rand, Rand 2026-11-01 (phase bo, after the lite hook was found to plan nonexistent commands); "all three" = the ATM task hooks task.assign, task.start, task.close (ADR-068); recorded 2026-11-01 replan session at Rand asking for anchors that verify written code against the diagrams with Jev, 2026-11-01. Status: locked. Version 3.

Summary: Jev: every code arm is in the diagram

Scope: adds or changes a lite task hook file or its state-transition diagram

Deterministic check to pair with the development gate: the per-hook Jev verification run reports every match arm as listed; the run JSON is committed under anchor-runs/

Fixtures:
- pass: The Jev run anchor-runs/hooks/task-close.json judges each of the six match arms in atm-bd-task-hook.py as listed in the task.close diagram.
- fail: The hook has a match arm for a refused close that the task.close diagram does not list.

## ANC-055

Anchor: The lite profile must not write or read a close-vars file or any other per-task state file.

> that is what I wanted done in the first place.

Said by Rand, phase-bo restart 2026-11-01 (fenix). Rand asked "how do we avoid: no close-vars file"; fenix answered: the task id is the bead id; every other fact is looked up at hook time from the bead (bd show), the provider by head branch (PR number, head, base), git/the stack bead (stack top), the phase file (profile), and the ATM task close request (outcome, report); the dev close is one atm task close command; an unlookupable fact makes the hook refuse with the exact fix command, never fall back to stored state. Rand replied with the quote., 2026-11-01. Status: locked. Version 1.

Summary: Lite has no close-vars or state file

Scope: adds or changes a lite task hook, lite dispatch template or the lite dev/fix/QA close procedure

Deterministic check to pair with the development gate: grep of skills/atm-bd-lite for close-vars, close_vars and file writes outside the shared runner is empty; no lite template asks for a vars file

## ANC-056

Anchor: The lite hooks must look up every fact other than the task id at hook time from bd, git, the provider, the phase file or the ATM request.

> that is what I wanted done in the first place.

Said by Rand, phase-bo restart 2 (2026-11-01, fenix-brief.md §A): the facts each hook looks up are the Facts tables of hooks/task-assign.md, task-start.md and task-close.md: phase file, cache.get(bead), cache.get(stack_bead), cache.get(id) per poured finding, and the ATM request. Reads go through beads-cache (ANC-063); no bd show, no provider or git lookup. An unlookupable fact makes the hook refuse with the fix from the Errors table. Original statement from Rand on "how do we avoid: no close-vars file"., 2026-11-01. Status: locked. Version 2.

Summary: Lite hooks look up facts at hook time

Scope: adds or changes a lite task hook, lite dispatch template or the lite dev/fix/QA close procedure

Deterministic check to pair with the development gate: each hooks/*.md has a Facts table whose every row is phase file, cache.get(...) or ATM request; the gate (ANC-066) finds no subprocess, bd or gh in the hook files

## ANC-057

Anchor: A lite dev or fix closes its task with one atm task close command carrying its report as the close message.

> that is what I wanted done in the first place.

Said by Rand, phase-bo restart 2026-11-01 (fenix). Rand asked "how do we avoid: no close-vars file"; fenix answered: the task id is the bead id; every other fact is looked up at hook time from the bead (bd show), the provider by head branch (PR number, head, base), git/the stack bead (stack top), the phase file (profile), and the ATM task close request (outcome, report); the dev close is one atm task close command; an unlookupable fact makes the hook refuse with the exact fix command, never fall back to stored state. Rand replied with the quote., 2026-11-01. Status: locked. Version 1.

Summary: Lite dev close is one atm task close

Scope: adds or changes a lite task hook, lite dispatch template or the lite dev/fix/QA close procedure

Deterministic check to pair with the development gate: the lite dev and fix templates contain exactly one close instruction, atm task close <bead id> completed with the report via --stdin

## ANC-058

Anchor: When a lite hook cannot look up a fact, it must refuse with the exact command that fixes it, never fall back to stored state.

> that is what I wanted done in the first place.

Said by Rand, phase-bo restart 2026-11-01 (fenix). Rand asked "how do we avoid: no close-vars file"; fenix answered: the task id is the bead id; every other fact is looked up at hook time from the bead (bd show), the provider by head branch (PR number, head, base), git/the stack bead (stack top), the phase file (profile), and the ATM task close request (outcome, report); the dev close is one atm task close command; an unlookupable fact makes the hook refuse with the exact fix command, never fall back to stored state. Rand replied with the quote., 2026-11-01. Status: locked. Version 1.

Summary: Missing fact means refuse with the fix

Scope: adds or changes a lite task hook, lite dispatch template or the lite dev/fix/QA close procedure

Deterministic check to pair with the development gate: every Errors-table row of the lite hook diagrams names a fix command; a test removes each lookup's data and asserts the refused reply text

## ANC-059

Anchor: The lite profile must have no repair, backfill or legacy-compatibility script.

> that is what I wanted done in the first place.

Said by Rand, phase-bo restart 2026-11-01 (fenix). Rand asked "how do we avoid: no close-vars file"; fenix answered: the task id is the bead id; every other fact is looked up at hook time from the bead (bd show), the provider by head branch (PR number, head, base), git/the stack bead (stack top), the phase file (profile), and the ATM task close request (outcome, report); the dev close is one atm task close command; an unlookupable fact makes the hook refuse with the exact fix command, never fall back to stored state. Rand replied with the quote., 2026-11-01. Status: locked. Version 1.

Summary: Lite has no repair or backfill scripts

Scope: adds or changes a lite task hook, lite dispatch template or the lite dev/fix/QA close procedure

Deterministic check to pair with the development gate: find skills/atm-bd-lite -name '*repair*' -o -name '*backfill*' -o -name '*legacy*' is empty

## ANC-060

Anchor: atm-bd-lite scripts, hooks and templates must not invoke gh pr list, gh pr checks, gh run watch or any gh command with --watch.

> make a anc which prohibits prohibited git/gh calls (which burn a lot of tokens)

Said by Rand, phase-bo restart 2 (2026-11-01, fenix-brief.md §A). The host gh wrapper (~/.local/bin/gh) blocks gh pr list, gh pr checks, gh run watch and any --watch flag because they poll and drain the shared GitHub rate limit. Lite hooks run no gh command at all (ANC-066); layer facts come from the stack bead., 2026-11-01. Status: locked. Version 4.

Summary: Lite never calls wrapper-blocked gh commands

Scope: adds or changes a lite script, hook, template or test that runs gh

Deterministic check to pair with the development gate: grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch' skills/atm-bd-lite is empty

Fixtures:
- pass: The lite hooks run no gh command; layer facts (branch, head, pr, base) come from cache.get(stack_bead). grep of skills/atm-bd-lite for gh pr list, gh pr checks, gh run watch and --watch is empty.
- fail: The lite QA template tells the agent to wait for CI with gh pr checks --watch.

## ANC-061

Anchor: atm-bd-types must not define profile names: no profile enum and no profile string literals; a profile is a name read from the phase file.

> the profile names should not be hardcoded in the crate | we can create as many profiles as we want.

Said by Rand, phase-bo replan session 2026-11-01 (fenix); atm-bd-types 0.1.1 has enum Profile {Full, Lite}, which this rules out, 2026-11-01. Status: locked. Version 1.

Summary: No profile names hard-coded in atm-bd-types

Scope: adds or changes code in atm-bd-types

Deterministic check to pair with the development gate: grep of crates/atm-bd-types and its generated .pyi for an enum or literal named lite, orchestration or full used as a profile is empty

Fixtures:
- pass: crates/atm-bd-types/src/lib.rs has `pub struct ProfileName(String)` parsed from the phase file; grep for "lite" and "orchestration" in the crate is empty.
- fail: crates/atm-bd-types/src/lib.rs has `pub enum Profile { Orchestration, Lite }`.

## ANC-062

Anchor: atm-bd-lite code must not pass or hold hook input, bead data, phase-file data or replies as dict, Any or untyped JSON; it uses the atm-bd-types types and traits.

> why are we passing a dictionary into the task-start hook call. that is red-flag #1 | a dictionary is a great way to get around pyright and using rust traits/types we defined in crate

Said by Rand, phase-bo replan 2026-11-01 (fenix); first hook drafts took dict[str, Any] envelopes, 2026-11-01. Status: locked. Version 1.

Summary: No dicts or Any in lite code; use atm-bd-types

Scope: adds or changes atm-bd-lite code

Deterministic check to pair with the development gate: grep of skills/atm-bd-lite for 'dict[', 'Any', 'json.loads' outside atm-bd-types parsing is empty; pyright strict reports no reportUnknown* diagnostics

Fixtures:
- pass: def hook(req: HookRequest) -> HookReply: ... bead = Bead.show(req.task_id)
- fail: def hook(env: dict[str, Any]) -> dict[str, str]: task = env["request"]["task_id"]

## ANC-063

Anchor: Every atm-bd-lite hook, script and agent step must read and write beads only through beads-cache (bd_cache_rs) and must never run bd directly.

> we have found 20-30 second timeout is not enough ... that is why we designed bd cache which you forgot is a mandatory requirement

Said by Rand, Rand 2026-11-01, phase bo replan, after the lite plan was found to call bd (BdCli) inside 2.5 s hooks; locked on Rand's 'lock it', 2026-11-01. Status: locked. Version 2.

Summary: lite beads access only via beads-cache

Scope: any lite hook, script, template step or adapter that reads or writes beads

Deterministic check to pair with the development gate: no bd subprocess call and no BdCli in any lite file or template; BeadStore is the bd_cache_rs adapter

Fixtures:
- pass: The lite close hook reads the bead with cache.get from bd_cache_rs and closes it with cache.write(bead, 'close', ...); no bd process is started.
- fail: The lite close hook runs 'bd show' and 'bd update --if-status' through a BdCli subprocess adapter.

## ANC-064

Anchor: A lite hook must make at most one beads-cache write and must never create, claim or release a bead.

> ANC-064 Gate, not sequence. A lite hook makes at most one beads-cache write. Check: count of `beads.update(`/`beads.create(`/`beads.claim(`/`beads.release(` call sites per hook file ≤ 1, and `create`/`claim`/`release` count = 0. Fail fixture: a close hook that calls `beads.update` twice (record layer, then close).

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: one beads-cache write per hook

Scope: any lite hook file

Deterministic check to pair with the development gate: atm-bd-lite-gate.py (ast): per hook file in skills/atm-bd-lite/scripts/atm-bd-task-*.py, count of beads.update(/beads.create(/beads.claim(/beads.release( call sites <= 1 and create/claim/release count == 0

Fixtures:
- pass: The lite close hook calls beads.update once, closing the bead with the layer evidence; it has no beads.create, beads.claim or beads.release call.
- fail: The lite close hook calls beads.update twice: first to record the layer on the stack bead, then to close the work bead.

## ANC-065

Anchor: A lite hook must never write the stack bead (the ledger).

> ANC-065 Hooks never write the ledger. Check: no hook file passes a stack bead to a write; grep for `stack_bead` in a hook file appears only as an argument to `beads.show(` / `cache.get(`. Fail fixture: `BeadPatch.record(stack_bead, …)` in any hook.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: hooks only read the ledger

Scope: any lite hook file

Deterministic check to pair with the development gate: atm-bd-lite-gate.py (ast): every stack_bead occurrence in skills/atm-bd-lite/scripts/atm-bd-task-*.py is an argument of beads.show( or cache.get(

Fixtures:
- pass: The lite close hook reads the ledger with cache.get(bead.stack_bead) and checks it holds a layer for work.branch; it passes stack_bead to no write.
- fail: The lite close hook calls beads.update(BeadPatch.record(stack_bead, layer)) to record the dev layer before closing.

## ANC-066

Anchor: A lite hook must not run a subprocess or use the BdCli, GitCli or AtmCli adapters, bd or gh.

> ANC-066 No subprocess in a hook. Check: grep for `subprocess`, `BdCli`, `GitCli`, `AtmCli`, `gh `, `bd ` in `skills/atm-bd-lite/scripts/atm-bd-task-*.py` is empty. Fail fixture: any of them.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: no subprocess in a hook

Scope: any lite hook file

Deterministic check to pair with the development gate: atm-bd-lite-gate.py (ast) on skills/atm-bd-lite/scripts/atm-bd-task-*.py: no import of subprocess; no reference to BdCli, GitCli or AtmCli; no call to os.system/os.popen/os.exec*/os.spawn*; no call whose first argument is a list or string starting with gh, bd or git

Fixtures:
- pass: The three lite hooks import only json, sys, atm_bd_types and bd_cache_rs; grep for subprocess, BdCli, GitCli, AtmCli, gh and bd in them is empty.
- fail: The lite close hook runs subprocess.run(["gh", "pr", "view", branch]) to read the PR head.

## ANC-067

Anchor: Every multi-step procedure a lite agent must run (stack, pour) must be a fenced bash block in a template under skills/atm-bd-lite/templates/.

> ANC-067 Sequences live in templates. Every multi-step procedure an agent must run (stack, pour) is a fenced bash block in a template under `skills/atm-bd-lite/templates/`. Check: the dev template contains one block whose lines are, in order, bdc lock, git rebase, git push, bdc record; the QA template contains one block with bdc pour. Fail fixture: a Python file that performs those steps.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: stack and pour are template blocks

Scope: lite templates and any lite script that sequences agent steps

Deterministic check to pair with the development gate: the dev template under skills/atm-bd-lite/templates/ has exactly one fenced bash block whose command lines match, in order, bdc lock, git rebase, git push, bdc record; the QA template has one fenced bash block containing bdc pour; no .py file under skills/atm-bd-lite runs git rebase, git push or a bdc lock/record/pour

Fixtures:
- pass: The lite dev template has one fenced bash block: bdc lock the stack bead, git rebase onto the top, git push, bdc record the layer and release; the QA template has one fenced block that runs bdc pour per finding.
- fail: skills/atm-bd-lite/scripts/atm-bd-lite-stack.py takes the stack lock, rebases, pushes and records the layer for the dev.

## ANC-068

Anchor: The generated atm_bd_types stub must carry only names used by a lite hook or a hooks/*.md table.

> ANC-068 Stub carries only used names. Check: every `ErrorId` member, class and method in the generated `.pyi` is referenced by a hook file or a `hooks/*.md` table. Fail fixture: `STACK_CLAIM_LOST` with no caller.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: stub carries only used names

Scope: atm-bd-types and its generated .pyi

Deterministic check to pair with the development gate: every ErrorId member, class and method name in the generated atm_bd_types .pyi appears (grep -w) in a lite hook file or a hooks/*.md table

Fixtures:
- pass: Every ErrorId member in the atm_bd_types stub, including FINDINGS_NOT_POURED and STACK_NOT_RECORDED, appears in a hooks/*.md Errors table or a hook file.
- fail: The stub keeps ErrorId.STACK_CLAIM_LOST although no hook and no hooks/*.md table names it.

## ANC-069

Anchor: The stack bead must be the ledger: its assignee is the lock, its layers are the record, and only the fenced stack block writes it, through bdc, under the lock.

> ANC-069 Stack bead is the ledger. Its assignee is the lock; its layers are the record; only the fenced stack block writes it, through bdc, under the lock. Check: ANC-065 plus the ANC-067 block shape.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 3.

Summary: stack bead is the atomic ledger

Scope: anything that reads or writes the stack bead

Deterministic check to pair with the development gate: the ANC-065 gate check passes on every lite hook and the ANC-067 dev-template block check passes

Fixtures:
- pass: The stack bead's assignee is the lock and its layers are the record: the dev template's fenced stack block runs bdc lock (sets the assignee), rebases, pushes, then bdc record appends the layer and releases; no hook or script writes the stack bead.
- fail: A cron script appends missing layers to the stack bead without taking the lock.

## ANC-070

Anchor: Each lite hook file must be at most 100 lines, including header and docstring.

> ANC-070 Line ceiling. Each hook file ≤ 100 lines including header and docstring. Check: `wc -l`. Fail fixture: 101 lines. This is the cheapest creep detector there is; a stack claim does not fit.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 4.

Summary: hook file <= 100 lines

Scope: any lite hook file

Deterministic check to pair with the development gate: wc -l skills/atm-bd-lite/scripts/atm-bd-task-*.py reports <= 100 for each file (run by atm-bd-lite-gate.py)

Fixtures:
- pass: wc -l reports 78, 64 and 61 lines for the close, start and assign hooks.
- fail: The bo-10 sprint delivers a 101-line lite close hook file, exceeding the 100-line maximum for a hook file.

## ANC-071

Anchor: docs/plans/phase-bo/intent.md and docs/plans/phase-bo/hooks/*.md must change only in commits authored by Rand.

> ANC-071 Spec is owned. `intent.md` and `hooks/*.md` change only in a commit authored by Rand. Check: `git log --format=%ae -- docs/plans/phase-bo/intent.md docs/plans/phase-bo/hooks/` shows only rand@randlee.com. Fail fixture: a commit from fenix editing a Transitions table. If fenix or a reviewer thinks the spec is wrong, the output is a question to Rand, never an edit.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 2.

Summary: only Rand commits the spec

Scope: docs/plans/phase-bo/intent.md and docs/plans/phase-bo/hooks/

Deterministic check to pair with the development gate: git log --format=%ae -- docs/plans/phase-bo/intent.md docs/plans/phase-bo/hooks/ | sort -u prints only rand@randlee.com

Fixtures:
- pass: git log --format=%ae for intent.md and hooks/ prints only rand@randlee.com; fenix raised the T-CLOSE-04 question to Rand by ATM message.
- fail: A commit from fenix edits the Transitions table in hooks/task-close.md to add a STACK_HELD row.

## ANC-072

Anchor: A review finding that would add a write, an arm, a subprocess or a guard to a lite hook must be out of scope unless it cites the hooks/*.md row it implements.

> ANC-072 Review findings bind to the spec, not the code. A review finding that would add a write, an arm, a subprocess or a guard to a hook is out of scope unless it cites the `hooks/*.md` row it implements. The reply template is one line: "Not in <hook>.md; file against the spec (ANC-071)." Check: ANC-064/065/066/070 fail any such change regardless of what the review said. Fail fixture: solar round asks for a retry path; fenix adds an arm; gate fails on rows ≠ arms.

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 3.

Summary: review findings bind to the spec

Scope: any review-driven change to a lite hook

Deterministic check to pair with the development gate: atm-bd-lite-gate.py exits 0 on the changed hook (rows == arms, writes <= 1, ANC-065/066 greps, <= 100 lines, both Jev question sets)

Fixtures:
- pass: Solar asks for a retry arm in the close hook; fenix replies "Not in task-close.md; file against the spec (ANC-071)." and the hook is unchanged.
- fail: A review asks for a retry path that no hooks/task-close.md row lists; fenix adds a retry arm to the close hook without citing a row.

## ANC-073

Anchor: Every phase-bo anchor check must name a command (grep, wc, ast, pyright, the gate) or a Jev fixture pair.

> ANC-073 Every anchor check is mechanical. A new anchor whose check is prose ("is simple", "is maintainable") is rejected by plan-anchor. Check: each anchor's `check` names a command (grep, wc, ast, pyright, the gate) or a Jev fixture pair. Fail fixture: "Jev judges the hook is reasonable."

Said by Rand, phase-bo restart 2, docs/plans/phase-bo/fenix-brief.md (written by Rand 2026-11-01); added by fenix exactly as written, 2026-11-01. Status: proposed. Version 3.

Summary: anchor checks are mechanical

Scope: any anchor added to docs/plans/phase-bo/anchors.json

Deterministic check to pair with the development gate: for every non-retired anchor in anchors.json, deterministic_check names grep, wc, ast, pyright, git, a test, or atm-bd-lite-gate.py, or the anchor has at least one pass and one fail fixture

Fixtures:
- pass: Every non-retired phase-bo anchor's check names a command (grep, wc -l, ast, git log or atm-bd-lite-gate.py) or the anchor has a Jev pass/fail fixture pair; ANC-070's check is wc -l.
- fail: A new anchor's check reads: Jev judges the hook is reasonable.

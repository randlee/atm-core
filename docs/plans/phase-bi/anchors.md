# Anchors - phase-current

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `doc/plan/phase-current/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | atm-bd skill = install.py behavior | 2026-10-16 |
| ANC-002 | retired | skill fetched from main head by default; --ref overrides | 2026-10-16 |
| ANC-003 | locked | skill install prints ref, sha and app version | 2026-10-16 |
| ANC-004 | locked | phase ends with PyPI publish after TestPyPI rehearsal | 2026-10-16 |
| ANC-005 | locked | nothing here changes v0.16.1 | 2026-10-16 |
| ANC-006 | locked | plan lives as beads in this bd db | 2026-10-16 |
| ANC-007 | retired | atm-bd-console kept as alias | 2026-10-16 |
| ANC-008 | retired | no skill-set selector yet | 2026-10-16 |
| ANC-009 | locked | atm-bd cron --phase <x> replaces console --headless | 2026-10-16 |
| ANC-010 | locked | bare skill installs the bundled skills, offline | 2026-10-16 |
| ANC-011 | locked | skill x.y.z installs that release tag | 2026-10-16 |
| ANC-012 | locked | skill --latest installs main head | 2026-10-16 |
| ANC-013 | locked | skill --list shows what the repo offers | 2026-10-16 |
| ANC-014 | locked | --orchestration selects today's skill set | 2026-10-16 |
| ANC-015 | locked | claude agents of unknown model get an orange starburst | 2026-10-16 |
| ANC-016 | retired | codex agents get a glyph per model class | 2026-10-16 |
| ANC-017 | locked | qa and sanity icons are fixed by agent type | 2026-10-16 |
| ANC-018 | locked | PR list uses Rand's GH_THEME icons | 2026-10-16 |
| ANC-019 | locked | claude agents get a per-model icon and colour | 2026-10-16 |
| ANC-020 | locked | codex agents get a celestial icon and colour per model class | 2026-10-16 |
| ANC-021 | locked | QA status icons: jev, running, pass, fail | 2026-10-16 |
| ANC-022 | locked | atm-bd-console is dropped, not aliased | 2026-10-16 |
| ANC-023 | locked | agent icon wherever an agent name is shown | 2026-10-16 |

## ANC-001

Anchor: The atm-bd command must have a skill subcommand that installs the atm-bd-orchestration skill into a repository by running the same installer logic as plugins/atm-bd-orchestration/install.py.

> it would do the same thing as the install.py does now right?

Said by Rand, planning session 2026-10-17: let's anchor this behavior. (not --profile yet), 2026-10-16. Status: locked. Version 3.

Summary: atm-bd skill = install.py behavior

Scope: adds an atm-bd-console subcommand, touches install.py or the skill install path

Deterministic check to pair with the development gate: run 'atm-bd-console skill' and install.py against the same target repo and diff the installed file sets; atm-bd-console skill --help shows no --profile

## ANC-002

Anchor: By default the atm-bd skill subcommand must fetch the skills from the head of the main branch of the atm-bd-orchestration repository at run time, without a republish of the app, and a --ref option must select another branch, tag or commit.

> we really always want to upgrade to most recent.

Said by Rand, planning session 2026-10-17: head of main as default, agreed, 2026-10-16. Status: retired. Version 2.

Summary: skill fetched from main head by default; --ref overrides

Scope: changes how the skill subcommand locates skill files

Deterministic check to pair with the development gate: run atm-bd skill with network traces or a local clone and confirm the installed files equal main's head; run with --ref <branch> and confirm that branch's files

## ANC-003

Anchor: The atm-bd skill subcommand must print the git ref and commit sha it installed the skills from, together with the app's own version.

> the script can find the repo and get most recent skill though.

Said by Rand, planning session 2026-10-17: report line proposed with the main-head default, agreed, 2026-10-16. Status: locked. Version 1.

Summary: skill install prints ref, sha and app version

Scope: edits skill subcommand output

Deterministic check to pair with the development gate: run atm-bd skill and grep its output for the ref, a 40-hex sha and the app version

## ANC-004

Anchor: The phase must end with the app published to PyPI, and a TestPyPI rehearsal of that publish must precede the production publish.

> we can publish to pypi, that can be at end of phase.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 1.

Summary: phase ends with PyPI publish after TestPyPI rehearsal

Scope: adds a publish or release sprint, touches pyproject.toml packaging

Deterministic check to pair with the development gate: pip index versions atm-bd lists the release; a TestPyPI release exists with an earlier upload time

## ANC-005

Anchor: No sprint in this phase must change what ships in v0.16.1.

> agreed 0.16.1 first, but we can plan this as part of next phase.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 2.

Summary: nothing here changes v0.16.1

Scope: any sprint dispatch, any change to the v0.16.1 release branch

Deterministic check to pair with the development gate: git diff v0.16.1..<phase base> shows no change from this phase's PRs; tag v0.16.1 predates the first sprint's first commit

## ANC-006

Anchor: The phase plan must exist as beads in the atm-core bd database with a phase root and sprint containers, and the phase must not use a markdown plan or sprint documents.

> we are creating plans as bd on this bd db.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 1.

Summary: plan lives as beads in this bd db

Scope: adds plan documents, creates phase or sprint beads

Deterministic check to pair with the development gate: bd show atm-phase-bi returns the root with sprint children; no docs/plans/phase-bi/sprint-*.md exists

## ANC-007

Anchor: The atm-bd-console command must remain available as an alias of the atm-bd command.

> should we just change the name of the cli to 'atm-bd'

Said by Rand, planning session 2026-10-17: rename to atm-bd with the old name kept, agreed, 2026-10-16. Status: retired. Version 2.

Summary: atm-bd-console kept as alias

Scope: edits [project.scripts] or renames the package or command

Deterministic check to pair with the development gate: after uv tool install, both atm-bd --help and atm-bd-console --help succeed with identical output

## ANC-008

Anchor: The atm-bd skill subcommand must not offer a skill-set selector option such as --profile.

> (not --profile yet)

Said by Rand, planning session 2026-10-17: let's anchor this behavior. (not --profile yet), 2026-10-16. Status: retired. Version 2.

Summary: no skill-set selector yet

Scope: adds an option to the skill subcommand

Deterministic check to pair with the development gate: atm-bd skill --help lists no --profile, --orchestration or --lite option

## ANC-009

Anchor: The single headless orchestration tick must be run as atm-bd cron --phase <x>, and the console's --headless option must be removed.

> atm-bd cron --phase <x>

Said by Rand, planning session 2026-10-17: cron is unused, so rename it to fit the atm-bd layout instead of leaving it, 2026-10-16. Status: locked. Version 1.

Summary: atm-bd cron --phase <x> replaces console --headless

Scope: edits the headless tick, the console CLI options, or the README cron examples

Deterministic check to pair with the development gate: atm-bd cron --help succeeds; atm-bd console --help lists no --headless; rg -- '--headless' apps/ plugins/ returns nothing

## ANC-010

Anchor: The atm-bd skill subcommand with no version or --latest argument must install the skills packaged inside the installed atm-bd package, without network access.

> atm-bd skill (bare would use skill in uv)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 1.

Summary: bare skill installs the bundled skills, offline

Scope: changes where the skill subcommand reads skill files or how the wheel is built

Deterministic check to pair with the development gate: build the wheel, unzip it and find the skill files; run atm-bd skill with the network disabled and confirm the install succeeds

## ANC-011

Anchor: The atm-bd skill subcommand given a version x.y.z must install the skills from the vX.Y.Z tag of the atm-bd-orchestration repository.

> atm-bd skill x.y.z (install version x.y.z)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 1.

Summary: skill x.y.z installs that release tag

Scope: adds or changes the version argument of the skill subcommand

Deterministic check to pair with the development gate: run atm-bd skill 0.16.0 and diff the installed files against git show v0.16.0

## ANC-012

Anchor: The atm-bd skill subcommand given --latest must install the skills from the head of the main branch of the atm-bd-orchestration repository.

> atm-bd --latest (latest on repo)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 1.

Summary: skill --latest installs main head

Scope: adds or changes the --latest option

Deterministic check to pair with the development gate: run atm-bd skill --latest and diff the installed files against git show origin/main

## ANC-013

Anchor: The atm-bd skill subcommand given --list must print the skill releases and skill sets available in the atm-bd-orchestration repository and must install nothing.

> atm-bd skill --list (shows what is available on repo)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 1.

Summary: skill --list shows what the repo offers

Scope: adds or changes the --list option

Deterministic check to pair with the development gate: run atm-bd skill --list, compare its releases to git tag -l on the repo, and confirm the target repo is unchanged

## ANC-014

Anchor: The atm-bd skill subcommand must accept an --orchestration option that selects the current atm-bd-orchestration skill set.

> --orchestration (current set of packages)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 1.

Summary: --orchestration selects today's skill set

Scope: adds or changes skill-set options on the skill subcommand

Deterministic check to pair with the development gate: atm-bd skill --help lists --orchestration; atm-bd skill --orchestration installs the same files as bare atm-bd skill

## ANC-015

Anchor: The console must show an orange starburst icon next to the name of every Claude agent whose model is not haiku, sonnet, opus or fable.

> orange starburst for claude (dev)

Said by Rand, planning session 2026-10-17: agent icons added to phase-bi, 2026-10-16. Status: locked. Version 2.

Summary: claude agents of unknown model get an orange starburst

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render a roster with a claude-code dev member and assert the line contains the starburst glyph with an orange style

## ANC-016

Anchor: The console must show ☀️ for Codex agents of model sol, 🌙 for luna, 🌍 for terra and ✨ for astra, next to the agent name.

> for codex what about ☀️, 🌙, 🌍 for sol,luna, terra and astra: ✨

Said by Rand, planning session 2026-10-17: agent icons added to phase-bi, 2026-10-16. Status: retired. Version 2.

Summary: codex agents get a glyph per model class

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render a roster with codex-cli members of each model class and assert each glyph appears next to its member

## ANC-017

Anchor: The console must show one fixed icon for every agent whose agent type is qa and one fixed icon for every agent whose agent type is sanity, regardless of its harness or model.

> I would like qa/sanity to have fixed icons, it would be good to specify by agent type

Said by Rand, planning session 2026-10-17: agent icons added to phase-bi, 2026-10-16. Status: locked. Version 1.

Summary: qa and sanity icons are fixed by agent type

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render qa and sanity members with differing harness and model and assert the same icon each

## ANC-018

Anchor: The console's pull-request list must show the CI pass, CI fail, running, merged and closed states with the glyphs and colours Rand supplied in GH_THEME.

> here are gh icons:

Said by Rand, planning session 2026-10-17: GH_THEME pasted (source-branch green/red, source-merge purple, cancel red, weather-hurricane blue), 2026-10-16. Status: locked. Version 1.

Summary: PR list uses Rand's GH_THEME icons

Scope: edits how pull-request states are rendered

Deterministic check to pair with the development gate: render a PR list with one PR per state and assert each line contains its GH_THEME code point and colour

## ANC-019

Anchor: The console must show, next to every Claude agent whose model is haiku, sonnet, opus or fable, that model's icon in that model's colour: U+F0156 in #FF6533 for haiku, U+F0382 in #FF784D for sonnet, U+F06C4 in #FF8C66 for opus and U+F0717 in #FF9F80 for fable.

> "haiku":  "[#D97757]\U000f0156[/]",

Said by Rand, planning session 2026-10-17: MODEL_ICONS pasted, 2026-10-16. Status: locked. Version 2.

Summary: claude agents get a per-model icon and colour

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render claude-code members with each model and assert each line has its code point and colour

## ANC-020

Anchor: The console must show, next to every Codex agent whose model class is sol, luna, terra or astra, that class's icon in that class's colour: U+F05A8 in #FFA60D for sol, U+F0F65 in #FFE066 for luna, U+F01E7 in #2DE057 for terra and U+F0674 in #FFE066 for astra.

> "astra": "[#FFE066]\U000f0674[/]",

Said by Rand, planning session 2026-10-17: CELESTIAL pasted, replacing the emoji for codex, 2026-10-16. Status: locked. Version 1.

Summary: codex agents get a celestial icon and colour per model class

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render codex-cli members of each model class and assert each line has its code point and colour

## ANC-021

Anchor: The console must show a QA status icon of U+F01A6 in #E551BA for the jev state, and U+F029B in #4A5AF8 while QA is running, in #2EA043 when QA has passed and in #F85149 when QA has failed.

> "qa_running": "[#4A5AF8]\U000f029b[/]",

Said by Rand, planning session 2026-10-17: JEV_QA pasted; placement and the Jev icon still to be agreed, 2026-10-16. Status: locked. Version 2.

Summary: QA status icons: jev, running, pass, fail

Scope: edits how QA state is rendered

Deterministic check to pair with the development gate: render QA states running, pass and fail and assert U+F029B with each colour

## ANC-022

Anchor: After installation no atm-bd-console command must exist, and no script, document, test or workflow in the atm-bd-orchestration repository may invoke atm-bd-console.

> jes drop atm-bd-console

Said by Rand, planning session 2026-10-17: answer to whether to drop the alias, 2026-10-16. Status: locked. Version 1.

Summary: atm-bd-console is dropped, not aliased

Scope: edits the command name, install scripts or any caller of the console

Deterministic check to pair with the development gate: after uv tool install, command -v atm-bd-console finds nothing; rg atm-bd-console over scripts, docs, tests and workflows finds no invocation

## ANC-023

Anchor: Wherever the console shows an agent's name, it must show that agent's icon next to the name.

> when agents are shown in ui, across the board, let's show agent icon.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 1.

Summary: agent icon wherever an agent name is shown

Scope: edits any render path that prints an agent name

Deterministic check to pair with the development gate: list every render path that emits an agent name and assert the icon next to it in each

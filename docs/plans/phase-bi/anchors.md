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
| ANC-015 | retired | claude agents of unknown model get an orange starburst | 2026-10-16 |
| ANC-016 | retired | codex agents get a glyph per model class | 2026-10-16 |
| ANC-017 | locked | qa and sanity icons are fixed by agent type | 2026-10-16 |
| ANC-018 | locked | PR list uses Rand's GH_THEME icons (CI fail moved to ANC-030) | 2026-10-16 |
| ANC-019 | locked | claude agents get a per-model icon and colour | 2026-10-16 |
| ANC-020 | locked | codex agents get a celestial icon and colour per model class | 2026-10-16 |
| ANC-021 | locked | qa agent icon: running blue, pass green, fail red | 2026-10-16 |
| ANC-022 | locked | atm-bd-console is dropped, not aliased | 2026-10-16 |
| ANC-023 | locked | agent icon immediately left of every agent name | 2026-10-16 |
| ANC-024 | locked | model icon by lower-case contains | 2026-10-16 |
| ANC-025 | locked | unmatched model: sonnet on claude-code, terra on codex-cli | 2026-10-16 |
| ANC-026 | locked | sanity agent icon is the Jev icon | 2026-10-16 |
| ANC-027 | locked | bead statuses use the BEAD_STATUS icons | 2026-10-16 |
| ANC-028 | locked | icons for the remaining states | 2026-10-16 |
| ANC-029 | retired | merge conflict = merged glyph in red | 2026-10-16 |
| ANC-030 | locked | CI fail = single failing platform icon in red, else red bug | 2026-10-17 |
| ANC-031 | locked | merge conflict = source-branch glyph in red | 2026-10-17 |

## ANC-001

Anchor: The atm-bd command must have a skill subcommand that installs the atm-bd-orchestration skill into a repository by running the same installer logic as plugins/atm-bd-orchestration/install.py.

> it would do the same thing as the install.py does now right?

Said by Rand, planning session 2026-10-17: let's anchor this behavior. (not --profile yet), 2026-10-16. Status: locked. Version 5.

Summary: atm-bd skill = install.py behavior

Scope: adds an atm-bd skill subcommand, touches install.py or the skill install path

Deterministic check to pair with the development gate: run 'atm-bd skill --dest R1' and install.py --dest R2 against a scratch repo and diff -r the installed file sets; atm-bd skill --help shows no --profile

Fixtures:
- pass: Adds a skill subcommand to atm-bd whose code calls the functions of plugins/atm-bd-orchestration/install.py to install the skills into a repository.
- fail: Adds a skill subcommand to atm-bd that carries its own copy of the install logic instead of calling install.py.

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

Said by Rand, planning session 2026-10-17: report line proposed with the main-head default, agreed, 2026-10-16. Status: locked. Version 2.

Summary: skill install prints ref, sha and app version

Scope: edits skill subcommand output

Deterministic check to pair with the development gate: run atm-bd skill and grep its output for the ref, a 40-hex sha and the app version

Fixtures:
- pass: After an install, atm-bd skill prints the source ref, the 40-character commit sha and the atm-bd version.
- fail: atm-bd skill installs the skills and prints only 'done'.

## ANC-004

Anchor: The phase must end with the app published to PyPI, and a TestPyPI rehearsal of that publish must precede the production publish.

> we can publish to pypi, that can be at end of phase.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 2.

Summary: phase ends with PyPI publish after TestPyPI rehearsal

Scope: adds a publish or release sprint, touches pyproject.toml packaging

Deterministic check to pair with the development gate: pip index versions atm-bd lists the release; a TestPyPI release exists with an earlier upload time

Fixtures:
- pass: The last sprint uploads the package to TestPyPI, checks the install, and then uploads it to PyPI.
- fail: The last sprint uploads the package straight to PyPI with no TestPyPI upload before it.

## ANC-005

Anchor: No sprint in this phase must change what ships in v0.16.1.

> agreed 0.16.1 first, but we can plan this as part of next phase.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 3.

Summary: nothing here changes v0.16.1

Scope: any sprint dispatch, any change to the v0.16.1 release branch

Deterministic check to pair with the development gate: git diff v0.16.1..<phase base> shows no change from this phase's PRs; tag v0.16.1 predates the first sprint's first commit

Fixtures:
- pass: The sprint changes only the new atm-bd code and leaves everything v0.16.1 ships untouched.
- fail: The sprint edits the 0.16.1 release's contents to apply the new rename.

## ANC-006

Anchor: The phase plan must exist as beads in the atm-core bd database with a phase root and sprint containers, and the phase must not use a markdown plan or sprint documents.

> we are creating plans as bd on this bd db.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 2.

Summary: plan lives as beads in this bd db

Scope: adds plan documents, creates phase or sprint beads

Deterministic check to pair with the development gate: bd show atm-phase-bi returns the root with sprint children; no docs/plans/phase-bi/sprint-*.md exists

Fixtures:
- pass: The plan is a phase root bead with sprint container beads in the atm-core bd database and no markdown sprint docs.
- fail: The plan is a set of markdown sprint documents under docs/plans/phase-bi with no beads.

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

Said by Rand, planning session 2026-10-17: cron is unused, so rename it to fit the atm-bd layout instead of leaving it, 2026-10-16. Status: locked. Version 3.

Summary: atm-bd cron --phase <x> replaces console --headless

Scope: edits the headless tick, the console CLI options, or the README cron examples

Deterministic check to pair with the development gate: atm-bd cron --help succeeds; atm-bd console --help lists no --headless; rg -n -g '!**/tests/**' -g '!**/CHANGELOG.md' -- '--headless' apps plugins .github returns nothing; a test asserts atm-bd cron --headless is rejected

Fixtures:
- pass: Adds the atm-bd cron --phase <x> subcommand that runs one tick, and removes the --headless option from the console.
- fail: Keeps the --headless option on the console and adds no cron subcommand.

## ANC-010

Anchor: The atm-bd skill subcommand with no version or --latest argument must install the skills packaged inside the installed atm-bd package, without network access.

> atm-bd skill (bare would use skill in uv)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 2.

Summary: bare skill installs the bundled skills, offline

Scope: changes where the skill subcommand reads skill files or how the wheel is built

Deterministic check to pair with the development gate: build the wheel, unzip it and find the skill files; run atm-bd skill with the network disabled and confirm the install succeeds

Fixtures:
- pass: Bare atm-bd skill installs the skills packaged inside the installed package and works with the network disabled.
- fail: Bare atm-bd skill downloads the skills from GitHub and fails when offline.

## ANC-011

Anchor: The atm-bd skill subcommand given a version x.y.z must install the skills from the vX.Y.Z tag of the atm-bd-orchestration repository.

> atm-bd skill x.y.z (install version x.y.z)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 2.

Summary: skill x.y.z installs that release tag

Scope: adds or changes the version argument of the skill subcommand

Deterministic check to pair with the development gate: run atm-bd skill 0.16.0 and diff the installed files against git show v0.16.0

Fixtures:
- pass: atm-bd skill 0.16.0 installs the skills from the v0.16.0 tag of the atm-bd-orchestration repository.
- fail: atm-bd skill 0.16.0 installs the skills from the head of main.

## ANC-012

Anchor: The atm-bd skill subcommand given --latest must install the skills from the head of the main branch of the atm-bd-orchestration repository.

> atm-bd --latest (latest on repo)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 2.

Summary: skill --latest installs main head

Scope: adds or changes the --latest option

Deterministic check to pair with the development gate: run atm-bd skill --latest and diff the installed files against git show origin/main

Fixtures:
- pass: atm-bd skill --latest installs the skills from the head of the main branch of atm-bd-orchestration.
- fail: atm-bd skill --latest installs the skills bundled in the installed package.

## ANC-013

Anchor: The atm-bd skill subcommand given --list must print the skill releases and skill sets available in the atm-bd-orchestration repository and must install nothing.

> atm-bd skill --list (shows what is available on repo)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 2.

Summary: skill --list shows what the repo offers

Scope: adds or changes the --list option

Deterministic check to pair with the development gate: run atm-bd skill --list, compare its releases to git tag -l on the repo, and confirm the target repo is unchanged

Fixtures:
- pass: atm-bd skill --list prints the available skill releases and sets and writes nothing into the target repository.
- fail: atm-bd skill --list prints the releases and then installs the newest one.

## ANC-014

Anchor: The atm-bd skill subcommand must accept an --orchestration option that selects the current atm-bd-orchestration skill set.

> --orchestration (current set of packages)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 5.

Summary: --orchestration selects today's skill set

Scope: adds or changes skill-set options on the skill subcommand

Deterministic check to pair with the development gate: atm-bd skill --help lists --orchestration; atm-bd skill --orchestration installs the same files as bare atm-bd skill

Fixtures:
- pass: atm-bd skill accepts --orchestration and selects the current atm-bd-orchestration skill set.
- fail: atm-bd skill has no --orchestration option: its help lists only --dest and --codex, and passing --orchestration exits with an unknown-option error.

## ANC-015

Anchor: The console must show an orange starburst icon next to the name of every Claude agent whose model is not haiku, sonnet, opus or fable.

> orange starburst for claude (dev)

Said by Rand, planning session 2026-10-17: agent icons added to phase-bi, 2026-10-16. Status: retired. Version 3.

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

Said by Rand, planning session 2026-10-17: agent icons added to phase-bi, 2026-10-16. Status: locked. Version 2.

Summary: qa and sanity icons are fixed by agent type

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render qa and sanity members with differing harness and model and assert the same icon each

Fixtures:
- pass: A qa agent and a sanity agent each render one fixed icon whatever their harness or model.
- fail: A qa agent renders a different icon depending on whether its model is sonnet or opus.

## ANC-018

Anchor: The console's pull-request list must show the CI pass, running, merged and closed states with the glyphs and colours Rand supplied in GH_THEME; the CI fail glyph is governed by ANC-030.

> here are gh icons:

Said by Rand, planning session 2026-10-17: GH_THEME pasted (source-branch green/red, source-merge purple, cancel red, weather-hurricane blue), 2026-10-16. Status: locked. Version 4.

Summary: PR list uses Rand's GH_THEME icons (CI fail moved to ANC-030)

Scope: edits how pull-request states are rendered

Deterministic check to pair with the development gate: render a PR list with one PR per state and assert each pass, running, merged and closed line contains its GH_THEME code point and colour

Fixtures:
- pass: The PR list renders CI pass, running, merged and closed with the GH_THEME glyphs and colours, and CI fail per ANC-030.
- fail: The PR list renders CI pass, running, merged and closed with the old ✅, 🌀, ⑂ and ⊗ characters and uses no GH_THEME glyph or colour.

## ANC-019

Anchor: The console must show, next to every Claude agent whose model is haiku, sonnet, opus or fable, that model's icon in that model's colour: U+F0156 in #FF6533 for haiku, U+F0382 in #FF784D for sonnet, U+F06C4 in #FF8C66 for opus and U+F0717 in #FF9F80 for fable.

> "haiku":  "[#D97757]\U000f0156[/]",

Said by Rand, planning session 2026-10-17: MODEL_ICONS pasted, 2026-10-16. Status: locked. Version 3.

Summary: claude agents get a per-model icon and colour

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render claude-code members with each model and assert each line has its code point and colour

Fixtures:
- pass: A claude-code agent with model opus renders U+F06C4 in #FF8C66, and haiku, sonnet and fable render their listed glyphs and colours.
- fail: A claude-code agent with model opus renders the sonnet glyph in #FF784D.

## ANC-020

Anchor: The console must show, next to every Codex agent whose model class is sol, luna, terra or astra, that class's icon in that class's colour: U+F05A8 in #FFA60D for sol, U+F0F65 in #FFE066 for luna, U+F01E7 in #2DE057 for terra and U+F0674 in #FFE066 for astra.

> "astra": "[#FFE066]\U000f0674[/]",

Said by Rand, planning session 2026-10-17: CELESTIAL pasted, replacing the emoji for codex, 2026-10-16. Status: locked. Version 2.

Summary: codex agents get a celestial icon and colour per model class

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render codex-cli members of each model class and assert each line has its code point and colour

Fixtures:
- pass: A codex-cli agent of class terra renders U+F01E7 in #2DE057, and sol, luna and astra render their listed glyphs and colours.
- fail: A codex-cli agent of class sol renders U+F01E7 in #2DE057.

## ANC-021

Anchor: The console must show a qa agent with the icon U+F029B in #4A5AF8 while it is running QA, in #2EA043 after a QA pass and in #F85149 after a QA fail.

> "qa_running": "[#4A5AF8]\U000f029b[/]",

Said by Rand, planning session 2026-10-17: JEV_QA pasted; placement and the Jev icon still to be agreed, 2026-10-16. Status: locked. Version 4.

Summary: qa agent icon: running blue, pass green, fail red

Scope: edits how QA state is rendered

Deterministic check to pair with the development gate: render QA states running, pass and fail and assert U+F029B with each colour

Fixtures:
- pass: A qa agent renders U+F029B in #4A5AF8 while running QA, #2EA043 after a pass and #F85149 after a fail.
- fail: A qa agent renders U+F029B in #2EA043 while running QA.

## ANC-022

Anchor: After installation no atm-bd-console command must exist, and no script, document, test or workflow in the atm-bd-orchestration repository may invoke the atm-bd-console command; the directory apps/atm-bd-console and the module atm_bd_console are not that command.

> jes drop atm-bd-console

Said by Rand, planning session 2026-10-17: answer to whether to drop the alias, 2026-10-16. Status: locked. Version 3.

Summary: atm-bd-console is dropped, not aliased

Scope: edits the command name, install scripts or any caller of the console

Deterministic check to pair with the development gate: after uv tool install, command -v atm-bd-console finds nothing; rg atm-bd-console over scripts, docs, tests and workflows finds no invocation

Fixtures:
- pass: After the sprint, atm-bd-console is not installed and no script, document, test or workflow in the repository invokes it.
- fail: After the sprint, atm-bd-console remains as an alias that bead-queues still calls.

## ANC-023

Anchor: Wherever the console shows an agent's name, it must show that agent's icon immediately to the left of the name.

> when agents are shown in ui, across the board, let's show agent icon.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 4.

Summary: agent icon immediately left of every agent name

Scope: edits any render path that prints an agent name

Deterministic check to pair with the development gate: list every render path that emits an agent name and assert the icon is the character cell immediately before the name in each

Fixtures:
- pass: Every place an agent name is shown renders that agent's icon immediately to the left of the name.
- fail: The status bar, the DAG and the PR list assignee show the agent name with no icon next to it; only the dashboard rows show icons.

## ANC-024

Anchor: The console must choose an agent's model icon by lower-casing the roster model string and testing it with contains for sol, luna, terra, astra, haiku, sonnet, opus and fable.

> when you pull agent type from atm model, you will to a to lower and use contains to pull out sol, luna, haiku, ...

Said by Rand, planning session 2026-10-17: how the model is matched, 2026-10-16. Status: locked. Version 3.

Summary: model icon by lower-case contains

Scope: edits how the roster model is mapped to an icon

Deterministic check to pair with the development gate: feed models GPT-5.6-Terra, Claude-Opus-5-5, claude, codex and unknown to icon_for and assert terra, opus, sonnet, terra icons and the unknown fallback

Fixtures:
- pass: The model icon is chosen by lower-casing the roster model string and testing with contains for sol, luna, terra, astra, haiku, sonnet, opus and fable.
- fail: The model icon is chosen only when the roster model string equals the exact name 'sonnet' or 'opus'.

## ANC-025

Anchor: When the roster model string matches none of the model names, the console must use the sonnet icon for a claude-code agent and the terra icon for a codex-cli agent.

> if claude or codex show up there, probably use sonnet/terra as stand-in for not knowing.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: locked. Version 3.

Summary: unmatched model: sonnet on claude-code, terra on codex-cli

Scope: edits how the roster model is mapped to an icon

Deterministic check to pair with the development gate: feed models claude and codex to icon_for and assert the sonnet and terra icons

Fixtures:
- pass: A claude-code agent whose model string matches no name renders the sonnet icon, and a codex-cli agent renders the terra icon.
- fail: An agent whose model string matches no name renders a blank space.

## ANC-026

Anchor: The console must show every sanity agent with the icon U+F01A6 in #E551BA, whatever its harness or model.

> "jev":        "[#E551BA]\U000f01a6[/]",

Said by Rand, planning session 2026-10-17: jev is the sanity icon, followed by the three qa states, 2026-10-16. Status: locked. Version 2.

Summary: sanity agent icon is the Jev icon

Scope: edits how agent names are rendered

Deterministic check to pair with the development gate: render sanity members with differing harness and model and assert U+F01A6 in #E551BA each

Fixtures:
- pass: Every sanity agent renders U+F01A6 in #E551BA whatever its harness or model.
- fail: A sanity agent on codex-cli renders the terra icon.

## ANC-027

Anchor: The console must show each bead status in Rand's BEAD_STATUS table with that status's icon and colour: blocked, in_progress, deferred, closed, pinned, hooked, accepted, planned, proposed and ready.

> "blocked":     "[#FA8537]\U000f033e[/]",  # lock

Said by Rand, planning session 2026-10-17: BEAD_STATUS pasted; open not chosen yet, 2026-10-16. Status: locked. Version 5.

Summary: bead statuses use the BEAD_STATUS icons

Scope: edits how a bead or sprint status is rendered

Deterministic check to pair with the development gate: render one bead per status in the table and assert each glyph code point and colour; render open and assert it is unchanged

Fixtures:
- pass: Each of the ten bead statuses (blocked, in_progress, deferred, closed, pinned, hooked, accepted, planned, proposed, ready) renders the icon and colour listed for it in the BEAD_STATUS table.
- fail: Only blocked, in_progress and closed render icons from the BEAD_STATUS table; the other seven statuses render a plain dot, and no BEAD_STATUS table is used.

## ANC-028

Anchor: The console must show bead open with U+F0766 in #2D81E0, sprint not_ready with U+F137C in #D97757, PR open with U+F04C2 in #2D81E0, PR draft with U+F0CB6 in #FFA60D and CI none with U+F01D8 in #2D81E0.

> "bead_open":        "[#2D81E0]\U000f0766[/]",

Said by Rand, planning session 2026-10-17: REMAINING pasted, 2026-10-16. Status: locked. Version 4.

Summary: icons for the remaining states

Scope: edits how bead, sprint, PR or CI states are rendered

Deterministic check to pair with the development gate: render each of the five states and assert its code point and colour

Fixtures:
- pass: Bead open, sprint not_ready, PR open, PR draft and CI none render U+F0766 in #2D81E0, U+F137C in #D97757, U+F04C2 in #2D81E0, U+F0CB6 in #FFA60D and U+F01D8 in #2D81E0 respectively.
- fail: PR draft renders U+F04C2 in #2D81E0, the same glyph and colour as PR open, and sprint not_ready renders the PR open glyph too.

## ANC-029

Anchor: The console must show a merge check of conflict with the merged icon U+F062D in red #F85149.

> merge conflict would be merged (w/ red)

Said by Rand, planning session 2026-10-17; red taken as the GitHub red already in the theme, 2026-10-16. Status: retired. Version 3.

Summary: merge conflict = merged glyph in red

Scope: edits how the merge check is rendered

Deterministic check to pair with the development gate: render a merge check of conflict and assert U+F062D in #F85149

Fixtures:
- pass: A merge check of conflict renders the merged glyph U+F062D in #F85149.
- fail: A merge check of conflict renders U+F062D in #8957E5, the merged purple.

## ANC-030

Anchor: The console must show a CI fail as the failing platform's icon in #F85149 when exactly one platform failed (macOS U+F0035, Windows U+F05B3, Linux U+F033D), and as the bug U+F00E4 in #F85149 when more than one platform failed or a general CI action not tied to a platform failed.

> Only one of macOS, Windows or Linux failed: show that platform's icon in red. More than one platform failed: show the red bug. A general CI action failed, one that is not tied to a platform: show the red bug.

Said by Rand, layout C spec, docs/plans/phase-bi/atm-bd cron status table (layout C).md, ci fail rule; ruling relayed by team-lead 2026-10-18 (01M56A77Q0JAB9R9WFBHN67104): the spec wins over ANC-018 for the CI fail glyph because the old source-branch red looked like a closed GitHub PR item, 2026-10-17. Status: locked. Version 2.

Summary: CI fail = single failing platform icon in red, else red bug

Scope: edits how a CI fail is rendered in the status table, PR list and cron markdown

Deterministic check to pair with the development gate: render CI fails with one failing platform each and assert U+F0035, U+F05B3, U+F033D in #F85149; render a two-platform fail and a general action fail and assert U+F00E4 in #F85149

Fixtures:
- pass: A CI fail where only Windows failed renders U+F05B3 in #F85149; a fail where macOS and Linux both failed renders the bug U+F00E4 in #F85149; a failed general action renders U+F00E4 in #F85149.
- fail: A CI fail where only Windows failed renders the source-branch glyph U+F062C in #F85149, or a two-platform fail renders one platform's icon.

## ANC-031

Anchor: The console must show a merge check of conflict with the source-branch icon U+F062C in red #F85149.

> | merge-check conflict | source-branch | U+F062C | #F85149 |

Said by Rand, layout C spec, docs/plans/phase-bi/atm-bd cron status table (layout C).md, PR states table; ruling relayed by team-lead 2026-10-18 (01M56A77Q0JAB9R9WFBHN67104): the spec wins over ANC-029; Rand moved the glyph off source-merge because it looked like a closed GitHub PR item, 2026-10-17. Status: locked. Version 2.

Summary: merge conflict = source-branch glyph in red

Scope: edits how the merge check is rendered

Deterministic check to pair with the development gate: render a merge check of conflict and assert U+F062C in #F85149

Fixtures:
- pass: A merge check of conflict renders the source-branch glyph U+F062C in #F85149.
- fail: A merge check of conflict renders the merged glyph U+F062D in #F85149, or U+F062C in #2EA043.

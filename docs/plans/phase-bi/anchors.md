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
| ANC-032 | locked | ci cell on a merged or closed PR = last run on its head | 2026-10-18 |
| ANC-033 | locked | PR link = the fetched PullRequest.url | 2026-10-18 |
| ANC-034 | locked | unknown = ? in #2D81E0 | 2026-10-18 |
| ANC-035 | locked | state cell: active agent wins over sprint state | 2026-10-17 |
| ANC-036 | locked | one icon per active agent, none dropped | 2026-10-17 |
| ANC-037 | locked | ci cell = exactly one of pass, fail, running, none | 2026-10-17 |
| ANC-038 | locked | pr cell = state icon then [#n](url), or - with no PR | 2026-10-17 |
| ANC-039 | locked | atm-bd cron --md-table emits the layout C table | 2026-10-17 |
| ANC-040 | locked | spec table form: exact header and alignment row | 2026-10-17 |
| REQ-BI6-AC6 | locked | same snapshot, identical bytes | 2026-10-18 |
| ANC-041 | locked | ADR-065 applied to atm-bd: local 24h on screen, UTC Z in files | 2026-10-18 |
| ANC-042 | locked | ADR-066 D1/D2: constant sender atm-bd@tool, repo-config override only | 2026-10-18 |
| ANC-043 | locked | ADR-066 D3: env never the sender; ATM_TEAM is the target team at most | 2026-10-18 |
| ANC-044 | locked | ADR-067 D3: viewer = time + level + message verbatim from the structured log | 2026-10-18 |
| ANC-045 | locked | ADR-067 D1: no second diagnostics file | 2026-10-18 |
| ANC-046 | locked | ADR-067 D2: writer sets level and composes the one-line failure message | 2026-10-18 |
| ANC-047 | locked | ADR-067 D4: one failure, one entry | 2026-10-18 |
| ANC-048 | locked | bead .12: configurable poll interval, bulk queries, paging past 100 | 2026-10-18 |
| ANC-049 | locked | bead .12: live quota guard at 50 percent with a visible pause | 2026-10-18 |
| ANC-050 | locked | bead .10: agent row = sprint/fix id + bead title, truncated | 2026-10-18 |

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bi-5.group-anc-f1`: touches no command name, install script or console caller: a two-entry icons.py change under bi-5; the deletion of the ANC-022 sentence was tried first (run 2026-10-18T02:30Z still underspecified at relevant=yes 0.31), per question-tuning-workflow

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bi-5.group-anc-f1`: two-entry icon fix (conflict, CI fail) under bi-5; it touches no bead-status icon; ANC-027 is judged on bi-5 itself (satisfied 2026-10-18); the plan gate returned an unstable near-threshold verdict on this item

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

## ANC-032

Anchor: The status table's ci cell for a sprint whose shown PR is merged or closed must show the last CI run on that PR's head (pass, running or a fail per ANC-030), not none.

> I think showing the last ci run is fine for now. IF a pr was merged and ci was red, this is something that might be OK, but I would want to know.

Said by Rand, bi-6 open question B, Rand to fenix 2026-10-18; spec rows bi-1 and bi-19 show pass on merged PRs, 2026-10-18. Status: locked. Version 2.

Summary: ci cell on a merged or closed PR = last run on its head

Scope: the ci cell of the status table (status_table.py CI producers) for merged and closed PRs

Deterministic check to pair with the development gate: fixture with a merged PR whose last run failed renders a fail glyph per ANC-030; one whose last run passed renders U+F062C #2EA043; neither renders U+F01D8

Fixtures:
- pass: A sprint whose newest PR is merged and whose last CI run on that head passed renders U+F062C in #2EA043 in the ci cell; a merged PR whose last run failed on Windows only renders U+F05B3 in #F85149.
- fail: A sprint whose newest PR is merged renders the ci cell as none (U+F01D8) regardless of the last run on its head.

## ANC-033

Anchor: The status table's pr cell must link the PR number to the PR url GitHub returned for that PR (PullRequest.url); it must not derive the link from a base URL or call gh pr view per PR.

> github url should be fine. I assume it would work.

Said by Rand, bi-6 open question C, Rand to fenix 2026-10-18; replaces the spec's base-URL derivation text, 2026-10-18. Status: locked. Version 2.

Summary: PR link = the fetched PullRequest.url

Scope: the pr cell link of the markdown emitter and the Rich emitter

Deterministic check to pair with the development gate: markdown row for a PR with url U renders [#n](U); a PR with empty url renders #n unlinked; the stubbed gh records no pr view call

Fixtures:
- pass: The pr cell renders [#281](https://github.com/randlee/atm-bd-orchestration/pull/281) using the url returned by the GitHub fetch for PR 281, and the test's gh stub records no pr view call.
- fail: The pr cell builds the link from a configured base URL plus the number, or calls gh pr view for each PR to obtain the url.

## ANC-034

Anchor: The status table must render an unknown member (unknown sprint, ci or pr state, or GitHub data that is off, rate-limited or truncated) as the glyph ? in #2D81E0, never as the no-PR dash or the ci none glyph.

> Unknown data the ? you showed would be fine. I would lighten the shade a bit (use same blue we used for the other items in spec)

Said by Rand, bi-6 open question E, Rand to fenix 2026-10-18; #2D81E0 is the spec's blue for bead open, PR open and ci none, 2026-10-18. Status: locked. Version 2.

Summary: unknown = ? in #2D81E0

Scope: the unknown members of SprintState, CiState and PrState in the icon tables and both emitters

Deterministic check to pair with the development gate: icon table test asserts ('?', '#2D81E0') for each unknown member; a GhStatus-off fixture renders ? in both ci and pr cells and a Problems row

Fixtures:
- pass: SprintState.unknown, CiState.unknown and PrState.unknown each map to the glyph ? in #2D81E0, and a snapshot with GitHub data truncated renders ? in the ci and pr cells with a Problems row.
- fail: An unknown state renders ? in #F85149, or GitHub-off renders the no-PR dash in the pr cell.

## ANC-035

Anchor: The status table's state cell must show the icon of the agent working the sprint whenever one is active (a dev agent, severity or QA), and the sprint's own state icon only when no agent is on it; a blocked sprint with an active agent must show the agent, never the lock.

> If no agent is working the sprint, show the sprint's state icon (closed, blocked, ready, not\_ready, and the rest). Because an agent wins over state, a blocked sprint that has an agent on it shows the agent. The lock appears only when nobody holds the sprint.

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 3.

Summary: state cell: active agent wins over sprint state

Scope: the STATE producer order of the shared status table module (agents before sprint status)

Deterministic check to pair with the development gate: fixture: a blocked sprint with an active dev agent renders that agent's glyph, not U+F033E; the same sprint with no agent renders U+F033E #FA8537

Fixtures:
- pass: A blocked sprint with fable active renders the fable glyph in its state cell; after fable's task closes and no agent remains, the same sprint renders the lock U+F033E in #FA8537.
- fail: The state cell shows the sprint's own status icon first and an agent's icon only while the sprint is in_progress, so a blocked sprint with an active dev agent renders the lock U+F033E in #FA8537, not the agent.

## ANC-036

Anchor: When more than one agent is active on a sprint, the state cell must draw one icon per active agent, every agent drawn and none collapsed, truncated or dropped, so the cell widens by one glyph per extra agent.

> The state cell shows one icon per active agent (Rand 2026-10-18: every active agent is drawn, the column widens by one cell per extra agent), else one icon picked in this order:

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 2.

Summary: one icon per active agent, none dropped

Scope: the agents_working STATE producer and both emitters (markdown and Rich)

Deterministic check to pair with the development gate: fixture with two active agents renders both glyphs in the state cell and nothing else; a third active agent adds a third glyph

Fixtures:
- pass: A sprint with fable and quality-mgr both active renders the fable glyph and the gavel side by side in its state cell, two glyph cells wide.
- fail: A sprint with two active agents renders only the first agent's glyph, or an overflow marker in place of the second.

## ANC-037

Anchor: The status table's ci cell must hold exactly one icon for the sprint's shown PR head: pass U+F062C #2EA043, running U+F0898 #58A6FF, none U+F01D8 #2D81E0 only when no CI run exists for that head, fail per ANC-030, or ? per ANC-034 when the CI state is unknown.

> | ci | one icon | pass, fail, running, none |

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 3.

Summary: ci cell = exactly one of pass, fail, running, none

Scope: the CI producers of the shared status table module

Deterministic check to pair with the development gate: fixtures: all checks passed renders U+F062C #2EA043; a run in progress renders U+F0898 #58A6FF; no run renders U+F01D8 #2D81E0; every ci cell is one glyph

Fixtures:
- pass: A sprint whose PR head's checks all passed renders U+F062C in #2EA043; one with a run still in progress renders U+F0898 in #58A6FF; one with no run on that head renders U+F01D8 in #2D81E0.
- fail: A sprint with a run still in progress renders the pass glyph, or a sprint with no run renders an empty ci cell or two glyphs.

## ANC-038

Anchor: The status table's pr cell must render the PR state icon (open, draft, merged or closed) followed by the PR number linked as [#n](url) per ANC-033, and exactly the dash - when the sprint has no PR.

> | pr | an icon and the PR number, or `-` when there is no PR | open, draft, merged, closed |

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 2.

Summary: pr cell = state icon then [#n](url), or - with no PR

Scope: the PR producers and the pr cell of the markdown emitter

Deterministic check to pair with the development gate: fixture: open PR 118 renders the open glyph then [#118](url); merged PR renders the merged glyph then the link; no PR renders exactly -

Fixtures:
- pass: A sprint with open PR 118 renders the open glyph then [#118](https://github.com/randlee/atm-bd-orchestration/pull/118); a sprint with no PR renders exactly -.
- fail: A sprint with no PR renders an empty pr cell or ?, or the PR number appears before the icon or without its link.

## ANC-039

Anchor: atm-bd cron --phase <x> --md-table must print the layout C markdown status table (form per ANC-040) to stdout.

> Command flag for the table output: `--md-table`, or `--md --table`.

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 4.

Summary: atm-bd cron --md-table emits the layout C table

Scope: the cron CLI flag and the markdown emitter entry point

Deterministic check to pair with the development gate: CLI test: cron --phase bi --md-table over a fixture snapshot prints to stdout a table whose first line is | sprint | state | ci | pr |

Fixtures:
- pass: Running atm-bd cron --phase bi --md-table prints the layout C markdown status table to stdout, starting with the line | sprint | state | ci | pr |.
- fail: atm-bd cron --phase bi --md-table is rejected as an unknown flag, or prints the dispatch summary instead of the layout C markdown table.

## ANC-040

Anchor: The emitted markdown table must use exactly the spec's header line | sprint | state | ci | pr | and alignment row |---|:-:|:-:|---|, with one row per sprint and a glyph in each icon slot.

> | sprint | state | ci | pr |
|---|:-:|:-:|---|
| bi-1 | <icon> | <icon> | <icon> [#118](https://github.com/<owner>/<repo>/pull/118) |

Said by Rand, bi-6 deliverable 11 (atm-bi-6.group-dev.1): anchor from the spec "atm-bd cron status table (layout C)" on develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf, Rand, Oct 17 2026, 2026-10-17. Status: locked. Version 4.

Summary: spec table form: exact header and alignment row

Scope: the markdown emitter's header, alignment row and row shape

Deterministic check to pair with the development gate: the first two emitted lines equal the spec's header and alignment row byte for byte; row count equals the sprint count

Fixtures:
- pass: The markdown emitter's first two lines are exactly | sprint | state | ci | pr | and |---|:-:|:-:|---|, followed by one row per sprint with a glyph in each icon slot.
- fail: The markdown emitter writes a header with different column names or a left-aligned alignment row such as |---|---|---|---| for the icon columns.

## REQ-BI6-AC6

Anchor: Rendering the markdown status table twice over the same snapshot must produce byte-identical output; no timestamp, dict ordering or run-dependent value may reach the emitted bytes.

> Running the emitter twice on the same snapshot gives identical bytes.

Said by atm-bi-6 plan, approved by Rand 2026-10-18 (plan review atm-phase-bi-plan-qa-2), split out of ANC-040 after anchor-tuning: the determinism rule comes from bi-6 acceptance criterion 6, not from the spec's words, 2026-10-18. Status: locked. Version 2.

Summary: same snapshot, identical bytes

Scope: the markdown emitter (AC6 byte test)

Deterministic check to pair with the development gate: AC6 byte test: a fixed snapshot renders to exactly the stored expected bytes; a second render compares equal byte for byte

Fixtures:
- pass: The AC6 byte test renders a fixed snapshot to the stored expected bytes and a second render of the same snapshot compares equal byte for byte.
- fail: Two renders of the same snapshot differ because the emitter writes a generated-at timestamp or iterates an unordered set of agents.

## ANC-041

Anchor: Every time atm-bd prints for a person (console header, rows, PR sheet, Diagnostics view, cron digest) is 24-hour local time produced by the program's one display formatter; every time it writes to a file or log is UTC ISO-8601 with a Z suffix.

> all time 'under the hood' should be utc, displayed times are 24h local format.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-065 applied to atm-bd: local 24h on screen, UTC Z in files

Scope: every strftime/isoformat call in atm-bd render, app, prs, cli and log paths

Deterministic check to pair with the development gate: no datetime formatting for display outside the one formatter; log and snapshot timestamps end in Z

Fixtures:
- pass: The console header, rows, PR sheet and Diagnostics view all call the one local-time formatter; the snapshot and log timestamps are written as 2026-10-18T07:46:21Z.
- fail: The PR sheet prints checked 07:36 from a UTC datetime's strftime while the header prints 00:36 local.

## ANC-042

Anchor: atm-bd sends every ATM message as the constant address atm-bd@tool; the only override is an optional <identity>@<team> key in the repository's .claude/project/atm-bd-orchestration.yaml.

> we should hard-code the tool identity in the future. accepting ATM_TEAM or ATM_IDENTITY will most likely cause more confusion that it is worth. if there is any over-ride of <identity>@<team>, it should happen as an optional config veriable at the repo level, not environment variable.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-066 D1/D2: constant sender atm-bd@tool, repo-config override only

Scope: the sender identity of every atm send and, once atm-core allows it, atm task assign the console runs

Deterministic check to pair with the development gate: the sender constant exists; no flag or environment variable feeds it; the repo config key is optional

Fixtures:
- pass: The console's sender is the constant atm-bd@tool; a repository sets tool_identity: ops-bot@tool in .claude/project/atm-bd-orchestration.yaml and every send goes out as ops-bot@tool.
- fail: The console sends as the phase lead from the phase file, or takes the sender from a --as flag.

## ANC-043

Anchor: atm-bd must not take its sender address from ATM_IDENTITY or ATM_TEAM; ATM_TEAM may only select the team the tool executes against.

> ATM_TEAM environment variable CAN be used to identify what team the cli is executing against, it should NOT be used as the atm address the tool uses for sending messages.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-066 D3: env never the sender; ATM_TEAM is the target team at most

Scope: config loading and the subprocess environment the console builds

Deterministic check to pair with the development gate: no read of ATM_IDENTITY in the console's own config; ATM_TEAM, if read, flows only into the target team

Fixtures:
- pass: The console reads ATM_TEAM only to choose which team's tasks it lists and assigns; its sends still go out as atm-bd@tool.
- fail: The console sends as the value of ATM_IDENTITY, or sets the sender's team from ATM_TEAM.

## ANC-044

Anchor: The atm-bd Diagnostics view reads only the sc-observability log, extracts the timestamp (rendered 24-hour local) and the level, and prints the entry's message unchanged; it parses nothing else from the entry.

> the <fix> <error kind> <details> should just come from the log entries. i.e. I don't want the diagnostic viewer doing any special processing of structured log message besides time and error level extract.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-067 D3: viewer = time + level + message verbatim from the structured log

Scope: the Diagnostics tab's data source and rendering

Deterministic check to pair with the development gate: the view's source is the logger query; the render concatenates local time, level and message with no per-kind branching

Fixtures:
- pass: The Diagnostics tab queries the sc-observability log for warning and error events and prints 00:10:18 error: followed by the entry's message string unchanged.
- fail: The Diagnostics tab reads a diagnostics.log text file, or rebuilds the line from the entry's argv, code and stderr fields itself.

## ANC-045

Anchor: atm-bd keeps no diagnostics or error file outside the sc-observability log; the hand-built diagnostics.log and the code that writes it are removed entirely.

> remove the 'hand-built' test file completely.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-067 D1: no second diagnostics file

Scope: every file the console opens for append under .atm-bd other than the sc-observability log and the actions journal

Deterministic check to pair with the development gate: no reference to diagnostics.log or a diagnostics path remains in the package; grep is empty

Fixtures:
- pass: The package has no diagnostics.log path, no _record_error, and no open(..., 'a') for a diagnostics file; errors reach the Diagnostics tab only from the sc-observability log.
- fail: A diagnostics.log file is still written beside the sc-observability log, even if the view no longer reads it.

## ANC-046

Anchor: A failed command is logged at level error with the message <fix in backticks> <command words> [<exit code>] <first stderr line>, composed once by the writer; the command words are the executable and subcommand, not the full argument vector.

> 00:10:18 error: `check gh and retry` gh api graphql [1] gh: API rate limit already exceeded for user ID 8083956.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-067 D2: writer sets level and composes the one-line failure message

Scope: the sc-observability event writer in atm-bd and every error it records

Deterministic check to pair with the development gate: an error entry has level error and a non-empty message matching the form; a stubbed gh failure renders the example line

Fixtures:
- pass: A gh failure with exit 1 and stderr 'gh: API rate limit already exceeded for user ID 8083956.' is logged at level error with message `check gh and retry` gh api graphql [1] gh: API rate limit already exceeded for user ID 8083956.
- fail: The failure is logged at level info with message null, or the message carries the full GraphQL query text from argv.

## ANC-047

Anchor: A failure is logged once when it happens; a cached failure still on display (such as the last GitHub error kept with cached PR data) is a sheet status, never a new log entry per tick.

> remove the 'hand-built' test file completely.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: ADR-067 D4: one failure, one entry

Scope: the per-tick error collection in the model and runner

Deterministic check to pair with the development gate: ten ticks with one cached GitHub error produce one error entry

Fixtures:
- pass: A GitHub refresh fails once at 07:16; the log holds one error entry for it and the PR sheet shows the cached error status on later ticks.
- fail: Each tick re-logs the cached GitHub error, giving twelve identical error entries for one failure.

## ANC-048

Anchor: The GitHub poll interval of atm-bd defaults to 10 minutes and is configurable in the repository configuration and on the command line; one refresh fetches up to 100 PRs per GraphQL page with few fields and pages past 100 rather than dropping PRs.

> i think a poll interval for gh ~10 minutes would be fine for default as long as we are doing queries that return a lot of info w/ small token cost. remember we get >100 gh stack (> 100 pr) frequently on multiple projects at the same time.

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: bead .12: configurable poll interval, bulk queries, paging past 100

Scope: the GitHub refresh cadence and query shape in atm-bd

Deterministic check to pair with the development gate: default interval 600 s; a repo with 150 PRs yields 150 rows across two pages

Fixtures:
- pass: github_poll_s defaults to 600 and a repo value of 300 or --github-poll 300 changes the cycle; a repository with 150 PRs produces 150 PR rows from two GraphQL pages.
- fail: The poll cycle is a fixed constant, or a repository with 150 PRs shows only the newest 100 with a truncated flag.

## ANC-049

Anchor: Before each GitHub refresh atm-bd reads the live remaining quota for the resource its mode uses (graphql or core) via gh api rate_limit; when remaining is below the configured threshold (default 50 percent of the limit) it skips the refresh, keeps cached PR data, shows a paused status with the numbers and the local reset time, and refuses forced refreshes until the quota is back above the threshold.

> can you file another feature request to add the poll interval AND token budget(s) for gh? it should also stop polling if token drop below a threshold (start w/ 50%)

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: bead .12: live quota guard at 50 percent with a visible pause

Scope: the GitHub refresh gate, its status line and the forced-refresh path

Deterministic check to pair with the development gate: a stubbed rate_limit at 49 percent skips the refresh and shows the pause line with a local reset time; at 51 percent after reset the refresh runs

Fixtures:
- pass: With graphql remaining 2310 of 5000 the refresh is skipped, the PR sheet shows 'GitHub paused: 2310/5000 graphql remaining, below 50 percent, resets 08:14', a forced refresh is refused with the same line, and the first cycle after reset above 50 percent refreshes.
- fail: The console refreshes regardless of the remaining quota, or counts its own calls instead of reading gh api rate_limit.

## ANC-050

Anchor: Agent task rows in the atm-bd dashboard read <sprint or fix id>: <bead title>, truncated to the pane width; never <phase>: <atm message text>.

> this is a second fix request, related to the first. The first was the description appears to be <phase>: <atm-message> where it shoudl be <sprint/fix>: <bead-title>

Said by Rand, console lockdown 2026-10-18: Rand's rulings on beads atm-phase-bi.7/.9/.10/.12/.13, ADR-065/066/067, 2026-10-18. Status: locked. Version 2.

Summary: bead .10: agent row = sprint/fix id + bead title, truncated

Scope: the dashboard agent row renderer and its tag source

Deterministic check to pair with the development gate: a row for a fix bead on sprint bi-5 starts with bi-5 or the fix id and the bead title, cut with an ellipsis at the pane width

Fixtures:
- pass: An agent row for a fix bead reads bi-5: console notify send to team-lead fails as self-addressed… cut at the pane width.
- fail: An agent row reads phase-bi: followed by the full text of the ATM assignment message running past the pane edge.

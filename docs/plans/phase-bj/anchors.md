# Anchors - phase-bj

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bj/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | atm-bd skill = install.py behavior | 2026-10-16 |
| ANC-002 | retired | skill fetched from main head by default; --ref overrides | 2026-10-16 |
| ANC-003 | locked | skill install prints ref, sha and app version | 2026-10-16 |
| ANC-004 | locked | phase ends with PyPI publish after TestPyPI rehearsal | 2026-10-16 |
| ANC-005 | retired | nothing here changes v0.16.1 | 2026-10-16 |
| ANC-006 | locked | plan lives as beads in this bd db | 2026-10-16 |
| ANC-007 | retired | atm-bd-console kept as alias | 2026-10-16 |
| ANC-008 | retired | no skill-set selector yet | 2026-10-16 |
| ANC-009 | locked | atm-bd cron --phase <x> replaces console --headless | 2026-10-16 |
| ANC-010 | locked | bare skill installs the bundled skills, offline | 2026-10-16 |
| ANC-011 | locked | skill x.y.z installs that release tag | 2026-10-16 |
| ANC-012 | locked | skill --latest installs the latest published release | 2026-10-16 |
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
| ANC-051 | locked | sanity and qa assignments go out as the phase lead | 2026-10-18 |
| ANC-052 | locked | sanity assigned mechanically when a dev or fix bead closes | 2026-10-18 |
| ANC-053 | locked | qa assigned mechanically on sanity PASS | 2026-10-18 |
| ANC-054 | retired | lead ruling 2026-10-18: current stack top with empty-ledger fallback | 2026-10-18 |
| ANC-055 | locked | dev task open until sanity PASS; closed by the sanity hook | 2026-10-18 |
| ANC-056 | locked | sanity FAIL reopens the bead and sends sanity-rejected | 2026-10-18 |
| ANC-057 | locked | sanity round counter with max_sanity_rounds default 5 | 2026-10-18 |
| ANC-058 | locked | hook vars are the shared sc-compose payload validated by a header-only template | 2026-10-18 |
| ANC-059 | locked | rate limiting never stops the hook; --skip-checks is help-only | 2026-10-18 |
| ANC-060 | locked | dev and fix dispatch stay the lead's judgment | 2026-10-18 |
| ANC-061 | locked | poller checks bd ready then the ATM task before assigning | 2026-10-18 |
| ANC-062 | retired | stack linking advised, base alignment sufficient, lead owns hygiene | 2026-10-18 |
| ANC-063 | retired | pour validates projected phase graph before writes | 2026-10-18 |
| ANC-064 | retired | atm task transition hooks in atm-core, routed by role, failure keeps the task open | 2026-10-18 |
| ANC-065 | locked | sanity links the layer on PASS; first PASS wins the top, the other gets rebase-required | 2026-10-18 |
| ANC-066 | locked | sanity-stack subagent: one next-top layer per spawn, sequential; the race is its job | 2026-10-18 |
| ANC-067 | locked | stack bead: the whole stack as ledger, its claim as the lock, lead coordinates through it | 2026-10-18 |
| ANC-068 | locked | lock timeout: --timeout input, report the holder, never blocks the team, never a human | 2026-10-18 |
| ANC-069 | locked | lean mode: GraphQL out or token budget means alignment and no divergence only | 2026-10-18 |
| ANC-070 | locked | qa hook informs the lead only, PASS or FAIL | 2026-10-18 |
| ANC-071 | locked | max_sanity_rounds does not move on rebase-required | 2026-10-18 |
| ANC-072 | locked | sanity FAIL handled entirely by ATM: sanity-fail.xml.j2 lists exactly what was NOT complete and references the sprint | 2026-10-18 |
| ANC-073 | locked | atm-bd runs pages for sanity and quality: one line per run, newest first, JEV and LLM verdict and elapsed, dev name and icon | 2026-10-18 |
| ANC-074 | retired | lead ruling 2026-10-18: Jev client owns three retries, four attempts total | 2026-10-18 |
| ANC-075 | retired | Publish full-skill sc-sha manifest | 2026-10-18 |
| ANC-076 | retired | Verify installed skill files at install | 2026-10-18 |
| ANC-077 | retired | Report installed file integrity status | 2026-10-18 |
| ANC-078 | locked | Sprint-family beads carry branch | 2026-10-18 |
| ANC-079 | locked | Sprint-family beads carry worktree | 2026-10-18 |
| ANC-080 | locked | Sprint-family beads carry pr_target | 2026-10-18 |
| ANC-081 | locked | Sprint-family beads carry difficulty | 2026-10-18 |
| ANC-082 | locked | Every bead kind has a typed model | 2026-10-18 |
| ANC-083 | locked | Validate every bead in the phase | 2026-10-18 |
| ANC-084 | locked | Retry LLM call failures three times | 2026-10-18 |
| ANC-085 | retired | Show API failure reason and distinct icon | 2026-10-18 |
| ANC-086 | locked | No outage beads or refusal citations | 2026-10-18 |
| ANC-087 | locked | Manifest hashes every published skill file | 2026-10-18 |
| ANC-088 | locked | Installer verifies skill files against manifest | 2026-10-18 |
| ANC-089 | locked | one vars file = union of atm task and sc-compose bead template variables | 2026-10-18 |

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-5`: PyPI publication is phase-end work; bj-5 adds manifest and installation verification, not publication.

Fixtures:
- pass: The last sprint uploads the package to TestPyPI, checks the install, and then uploads it to PyPI.
- fail: The last sprint uploads the package straight to PyPI with no TestPyPI upload before it.

## ANC-005

Anchor: No sprint in this phase must change what ships in v0.16.1.

> agreed 0.16.1 first, but we can plan this as part of next phase.

Said by Rand, planning session 2026-10-17, 2026-10-16. Status: retired. Version 4.

Summary: nothing here changes v0.16.1

Scope: any sprint dispatch, any change to the v0.16.1 release branch

Deterministic check to pair with the development gate: git diff v0.16.1..<phase base> shows no change from this phase's PRs; tag v0.16.1 predates the first sprint's first commit

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships
- `atm-bj-3`: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships
- `atm-bj-4`: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships
- `atm-bj-1`: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships
- `atm-bj-5`: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the atm-bd skill install subcommand is not touched by bj-2

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

Anchor: The atm-bd skill subcommand given --latest must install the skills from the latest published release of the atm-bd-orchestration repository.

> atm-bd --latest (latest on repo)

Said by Rand, planning session 2026-10-17: skill sources redesigned, agreed (yes), 2026-10-16. Status: locked. Version 3.

Summary: skill --latest installs the latest published release

Scope: adds or changes the --latest option

Deterministic check to pair with the development gate: Run atm-bd skill --latest and diff the installed files against the files of the newest published release tag.

Fixtures:
- pass: atm-bd skill --latest installs the skills from the newest published release of atm-bd-orchestration.
- fail: atm-bd skill --latest installs the unreleased head of the main branch instead of the latest published release.

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

Anchor: The atm-bd-console command must be replaced by the atm-bd console subcommand.

> jes drop atm-bd-console
'atm-bd-console' is to be removed/replaced w/ atm-bd console
atm-bd console would logically be in atm-bd-console module
I think I was discussing subcommands 'atm-bd console', 'atm-bd install', 'atm-bd cron', ...  This is a natural partition of the product.  all are atm-bd, different features are available.

Said by Rand, Rand 2026-10-19 ANC-022 ruling, 2026-10-16. Status: locked. Version 5.

Summary: atm-bd-console is dropped, not aliased

Scope: adds, renames or invokes an atm-bd subcommand (atm-bd console, atm-bd skill, atm-bd cron) or the old atm-bd-console command

Deterministic check to pair with the development gate: command -v atm-bd-console fails, atm-bd console runs, rg finds no atm-bd-console invocation outside apps/atm-bd-console and atm_bd_console

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bi-5.group-anc-f1`: touches no command name, install script or console caller: a two-entry icons.py change under bi-5; the deletion of the ANC-022 sentence was tried first (run 2026-10-18T02:30Z still underspecified at relevant=yes 0.31), per question-tuning-workflow
- `atm-bj-2`: bj-2 ships the stack bead schema, the pour metadata and validate-plan; it does not name, install, edit the installer for, or run any console command
- `atm-bj-3`: bj-3 ships agent prompts for sanity-stack and dev-sanity; it does not name, install or run any atm-bd subcommand

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes
- `atm-bj-1`: bj-1 edits the close hook script and its templates and renders no bead, sprint, PR or CI state in the console; ANC-028 scope is console rendering (relevance no at 0.56 and 0.79, leaning not_applicable, same applicability-noise class as the other phase-bi exclusions)

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: BJ-1 does not own console PR status-table rendering; it changes only the close hook and its templates.

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table and its markdown export are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's
- `atm-bj-3`: atm-bd's sender address and logging are atm-bd tool code; bj-3 edits agent prompts only
- `atm-bj-4`: atm-bd's sender address and logging are not touched by bj-4

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's
- `atm-bj-3`: atm-bd's sender address and logging are atm-bd tool code; bj-3 edits agent prompts only
- `atm-bj-4`: atm-bd's sender address is not touched by bj-4; the lead identity on dispatch's assign subprocess is ANC-051's and unchanged

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's
- `atm-bj-3`: atm-bd's log line format is atm-bd tool code; bj-3 edits agent prompts only
- `atm-bj-2`: BJ-2 plans bead schemas, formula producers and validation; it does not write or change the atm-bd observability event logger.

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: atm-bd's sender address and logging are atm-bd tool code; bj-3 edits agent prompts only
- `atm-bj-4`: atm-bd's sender address and logging are not touched by bj-4
- `atm-bj-1`: BJ-1 does not own cached console error logging or PR display; its scope is the close hook.

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's
- `atm-bj-3`: atm-bd's GitHub quota read is atm-bd tool code; bj-3 edits agent prompts only

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-4`: the status table, the dashboard task rows and their icons are phase-bi code that bj-4 does not edit; the runs tabs are two new panes

Fixtures:
- pass: An agent row for a fix bead reads bi-5: console notify send to team-lead fails as self-addressed… cut at the pane width.
- fail: An agent row reads phase-bi: followed by the full text of the ATM assignment message running past the pane edge.

## ANC-051

Anchor: Every atm task assign that the close hook or atm-bd issues for a sanity or qa bead must run as the phase lead named in .atm-bd/phase-<x>.toml, set on that one subprocess, while every notice atm-bd sends keeps the constant sender atm-bd@tool.

> we decided that tools would be atm-bd@tool, however, the task assignment needs to be assigned '--as <phase-lead>'

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: sanity and qa assignments go out as the phase lead

Scope: the sender of atm task assign in atm-task-close.py and in atm-bd dispatch; notices keep ANC-042

Deterministic check to pair with the development gate: grep the assign subprocess env in atm-task-close.py and atm-bd dispatch for ATM_IDENTITY set from the phase file lead; a notice send has no such override

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook
- `atm-bj-2`: BJ-2 plans bead formula and validation behavior only; the phase-lead identity of close-hook and dispatch assignments is implemented by BJ-1 and BJ-4.

Fixtures:
- pass: The hook assigns the sanity bead with ATM_IDENTITY=team-lead taken from .atm-bd/phase-bj.toml on that subprocess only; its dev-complete send goes out as the dev and atm-bd's notices as atm-bd@tool.
- fail: The hook assigns the sanity bead as the dev, or atm-bd assigns it as atm-bd@tool, or the lead identity comes from the process environment instead of the phase file.

## ANC-052

Anchor: When a dev or fix bead closes, the group's sanity bead must be assigned to the dev-sanity member with the sanity template and vars without any lead action: by the dev's close hook in the same shell command as bd close, and by atm-bd when bd ready lists the sanity bead with no ATM task.

> the first issue to solve is mechanical.  when dev or fix bd is closed, sanity 'bd ready' is true.  the atm-bd cron/console will 'atm task assign <bead-id> <sanity-agent> --template <template> --args <vars> automatically.  This assignment must be done --as <phase-lead>
the atm-task-close hook would immediately call 'atm task assign' and 'bd close'
yes, this is an intentional race condition.  In reality it is more of a backup plan

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: sanity assigned mechanically when a dev or fix bead closes

Scope: atm-task-close.py dev and fix roles; atm-bd dispatch of sanity beads

Deterministic check to pair with the development gate: close a dev bead through the hook in a scratch repo and read atm task list: the sanity task exists before the hook exits; kill the hook between its commands and one atm-bd tick assigns it

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the close hook's roles, checks, round counter and messages are bj-1; bj-2 ships the stack bead schema and the pour only
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook
- `atm-bj-4`: bj-4 plans only the atm-bd dispatch read order of ANC-061; the close-hook assignment path of ANC-052 is bj-1, which satisfies ANC-052, and the sealed ANC-052 addressed question requires both paths in one item (the two-path reading is the ANC-052 clarity item for Rand)

Fixtures:
- pass: The dev role of atm-task-close.py runs atm task assign atm-sanity --task-id <sanity bead> and bd close <dev bead> in one shell command; an atm-bd tick finds a ready sanity bead with no task and assigns it.
- fail: The lead reads the dev-complete report and runs atm task assign for the sanity bead by hand, or atm-bd only logs a notice that the sanity bead is ready.

## ANC-053

Anchor: When a sanity bead closes with PASS, the sanity close hook must assign the group's qa bead to the quality-mgr member with the qa template and vars as the phase lead, with no lead action.

> likewise, when a sanity-bd passes, the same task assignment occurs 'atm task assign <quality-agent> --template <qa-template> --args <vars> --as <phase-lead>

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: qa assigned mechanically on sanity PASS

Scope: atm-task-close.py sanity role on PASS

Deterministic check to pair with the development gate: run the sanity role with verdict PASS in a scratch repo and read atm task list: the qa task exists, assigned by the lead identity, before the hook exits

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: The sanity role on PASS runs atm task assign quality-mgr --task-id <qa bead> --template qa-template.xml.j2 --vars <file> with the lead identity on that subprocess.
- fail: On PASS the hook closes the sanity bead and the lead dispatches the qa bead on the next bd ready pass.

## ANC-054

Anchor: For dev and fix roles, task close must check only an open branch PR, pushed/PR head equality, and HEAD descent from the stack top (last layer, else pr_target); it must not check other layers or run lint or tests.

> there should be a switch on role, then depending on role, this will verify that the pr is created, that it is stacked on the layer below (I think this can be determined locally by verifying the base is the previous top of stack).  The checks should be minimal to insure rebase occurred, maintaining the entire stack is NOT dev agents responsibility.  We can not create conditions that it is impossible for the dev agent to complete due to unnecessary python complexity.

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: retired. Version 6.

Summary: lead ruling 2026-10-18: current stack top with empty-ledger fallback

Scope: atm-task-close.py dev/fix admission

Deterministic check to pair with the development gate: Tests verify these three admission checks, the empty-layer fallback, and that no other stack layers, lint, or tests are checked.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the close hook's roles, checks, round counter and messages are bj-1; bj-2 ships the stack bead schema and the pour only
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: The dev role checks an open branch PR, pushed and PR heads match, and HEAD descends from the last layer or pr_target when no layer exists; it checks no other layer and runs no lint or tests.
- fail: The dev role requires every stack layer to be coherent or runs a test suite before it can close.

## ANC-055

Anchor: The ATM task of a dev or fix bead must stay open after the dev's close hook runs and must be closed only by the sanity close hook on PASS, as the lead; the dev waits for that close or for the sanity-fail message and starts no other task meanwhile.

> I would NOT close the atm task until AFTER sanity is green.  This simplifies the dev<->sanity loop.  in the atm-task-close hook for sanity, if sanity pass, THEN atm task is closed (and dev can begin on next task in the queue), sanity task is closed

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 3.

Summary: dev task open until sanity PASS; closed by the sanity hook

Scope: the dev and fix roles never call atm task close completed; the sanity role does, on PASS, for the checked bead's task

Deterministic check to pair with the development gate: grep atm-task-close.py: no atm task close completed in the dev or fix role; the sanity PASS branch closes the checked bead's task with the lead identity; atm task events of a dev task show the close after the sanity PASS

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook
- `atm-bj-4`: the dev task's open and close lifecycle is the close hook's (bj-1); bj-4 reads runs and dispatches sanity and qa beads only
- `atm-bj-2`: BJ-2 ships stack schema, pour and validation only; whether the dev ATM task stays open through sanity is implemented by the BJ-1 close hook.

Fixtures:
- pass: After the dev role runs, atm task list still shows the dev task active; the sanity role on PASS closes it as the lead and the dev's next queued task starts.
- fail: The dev role closes the dev's ATM task with dev-complete.md.j2 and the dev starts its next queued task before sanity has run.

## ANC-056

Anchor: On a sanity FAIL below the round cap, the sanity close hook must reopen the checked bead to its assignee and send that dev the sanity-fail.xml.j2 template naming each undone deliverable, on the dev's still-open task; it must not create finding beads, a new layer or a new task.

> If sanity fails, dev bead is re-opened and 'atm send <dev-agent> --template <sanity-rejected-template>'

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 3.

Summary: sanity FAIL reopens the bead and sends sanity-rejected

Scope: atm-task-close.py sanity role on FAIL; templates/sanity-fail.xml.j2; retirement of sanity-create-findings and dev-fix.xml.j2

Deterministic check to pair with the development gate: run the sanity role with verdict FAIL in a scratch repo: the checked bead is open with its assignee, the dev received one sanity-fail message, bd list --parent <checked bead> is empty, no new ATM task exists

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook
- `atm-bj-2`: bj-2 plans the stack bead, the pour and validate-plan and does not write the sanity close hook or sanity-fail.xml.j2; the sanity FAIL path of ANC-056 is bj-1 (addressed yes at 0.70 then 0.55 on unchanged text, leaning satisfied, the near-threshold class)

Fixtures:
- pass: On FAIL the hook runs bd update <dev bead> --status open --assignee <dev> and atm send <dev> --template sanity-fail.xml.j2 --vars <file>; the dev fixes on the same branch.
- fail: On FAIL the hook creates a finding child per undone deliverable and the lead assigns dev-fix.xml.j2 on a new layer.

## ANC-057

Anchor: The sanity close hook must count FAIL rounds per sanity bead in metadata.sanity_round and, when the count exceeds max_sanity_rounds from the repository configuration (default 5 when unset), must escalate to team-lead instead of reopening the checked bead.

> sure, we can have sanity keep a counter and when count exceeded escelate to team-lead.  I would default max-sanity to ~5 if not overridden in atm-bd repo git settings.

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 3.

Summary: sanity round counter with max_sanity_rounds default 5

Scope: atm-task-close.py sanity role FAIL branch; .claude/project/atm-bd-orchestration.yaml key max_sanity_rounds rendered from registry.yaml

Deterministic check to pair with the development gate: six FAIL runs in a scratch repo with no setting leave sanity_round at 6, the dev bead closed-not-reopened on the sixth, and one SANITY.ROUND_CAP send to team-lead; five FAILs reopen the dev bead every time; a repo value of 2 escalates on the third

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the close hook's roles, checks, round counter and messages are bj-1; bj-2 ships the stack bead schema and the pour only
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: sanity_round goes 1 to 5 with the dev bead reopened each time; on the sixth FAIL the hook sends team-lead SANITY.ROUND_CAP and reopens nothing; max_sanity_rounds: 2 escalates on the third.
- fail: The hook escalates on the fifth FAIL with the default setting, or reopens the dev bead on every FAIL forever.

## ANC-058

Anchor: atm-task-close.py must take its input as one sc-compose --vars file that carries every value the role needs, must validate it by rendering a header-only j2 template in strict mode before any check runs, and must pass that same file unchanged to the atm task assign and atm send templates it runs.

> the hook has vars specified by the hook python.  the hook input vars should be sc-compose --vars payload with every setting necessary to complete everything inside the hook.  the input to the hook would verify vars against pydantic model and reject any missing/unpopulated vars you could define this as a 'special unrendered sc-compose j2 w/ only compined header' so validation is trivial.  (this would skip the need for a pydantic model).  the same vars could be sent to ... same vars sent to 'atm ... --template --vars <shared-vars>'  and sc-compose bead ... --template --vars <shared-vars>

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: hook vars are the shared sc-compose payload validated by a header-only template

Scope: the input contract of atm-task-close.py; templates/task-close-vars.j2

Deterministic check to pair with the development gate: a vars file missing commit fails before any git or gh call with an err naming commit; the assign and send subprocess argv carry --vars <the same path>

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: The hook renders task-close-vars.j2 (required_variables only) with sc-compose in strict mode and refuses a file missing pr_number; the same path is passed as --vars to atm task assign.
- fail: The hook defines a pydantic model of its own fields and builds a second vars dict for the assignment template.

## ANC-059

Anchor: atm-task-close.py must proceed with local git checks when GitHub is unreachable or rate limited, recording that in its result, and must offer a --skip-checks flag that bypasses the PR and stack checks, described only in the script's --help as requiring the phase lead's permission and named in no template or prompt.

> if we rate limit, we need to proceed.  we can't afford to stop due to rate limiting.  we should also provide a 'backdoor' flag the dev can use to get around checks WHEN given permissions.  i.e. --skip-checks.  --skip-checks wouldn't be in the templates or prompt prose, but could be discovered through help on the python script the dev is calling w/ description 'skips pr,.., checks. Do NOT use without permission from <lead>'  (or similar).

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: rate limiting never stops the hook; --skip-checks is help-only

Scope: GitHub failure handling and the --skip-checks flag of atm-task-close.py; the dev, fix, sanity and qa templates and agent prompts

Deterministic check to pair with the development gate: run the hook with GH_TOKEN unset: exit ok with github: unavailable in the result; grep -r skip-checks templates/ agents/ returns nothing; --help lists --skip-checks with the lead-permission sentence

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: With gh failing on rate limit the hook records github: unavailable, checks origin/<pr_target> locally and closes; --help shows '--skip-checks: skips the PR and stack checks. Do NOT use without permission from the phase lead' and no template mentions it.
- fail: A GitHub 403 makes the hook exit with an error and the dev task cannot close, or dev-template.xml.j2 tells the dev to pass --skip-checks when gh fails.

## ANC-060

Anchor: atm-bd must not assign dev or fix beads; the lead assigns them, the dev starts its next queued task when the sanity hook closes its current task, and a qa fix bead goes to the dev the lead picks at the finding's difficulty.

> dev assignment is not mechanical.  this works as it does now.  dev agent will start next queued task as soon as sanity pass.  a different dev will get qa fix beads at appropriate level set by lead.

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: dev and fix dispatch stay the lead's judgment

Scope: atm-bd dispatch: sanity and qa beads only; dev, fix and finding beads are the lead's

Deterministic check to pair with the development gate: atm-bd's actions.jsonl over a tick with a ready dev bead and no task holds no assign for it; the candidate filter in dispatch.py admits only sanity and qa stages

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: atm-bd dispatch and the lead's dev assignment are not touched by bj-3; its agents assign nothing
- `atm-bj-4`: atm-bd dispatch is not touched by bj-4; the runs tabs assign nothing
- `atm-bj-2`: bj-2 writes the pour metadata and the stack bead and does not touch atm-bd dispatch or any assignment; the dispatch candidate filter ANC-060 governs is unchanged (same ground as the ANC-061 exclusion)

Fixtures:
- pass: A tick with a ready dev bead and a ready sanity bead assigns the sanity bead and emits a notice for the dev bead; the lead picks the dev.
- fail: atm-bd assigns the ready dev bead to the first idle member of its difficulty tier.

## ANC-061

Anchor: atm-bd must assign a sanity or qa bead only when bd ready lists it and, checked afterwards, no ATM task with that bead id exists; a bead whose task the hook already assigned is never assigned a second time.

> There is the possibility of a race condition, however the atm-task-close hook would immediately call 'atm task assign' and 'bd close' we could even call 'atm task assign' before 'bd close'  (ideally these are both done in a single shell command so the delay is ~1ms, much faster than the cron/poller would see the problem.  We could insure the poller looked at 'bd ready' first and in the case 'bd ready' was true then check ... then check 'atm task' for assignment.  (this should avoid race condition).

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: locked. Version 2.

Summary: poller checks bd ready then the ATM task before assigning

Scope: atm-bd dispatch candidate filter order for sanity and qa beads

Deterministic check to pair with the development gate: dispatch.py reads bd ready first and atm task list second for each candidate; a tick against a sanity bead whose task the hook assigned emits no assign action

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: bj-1 does not touch atm-bd dispatch; the read order ANC-061 names is bj-4's deliverable 8
- `atm-bj-2`: bj-2 does not touch atm-bd dispatch; the read order ANC-061 names is bj-4's deliverable 8

Fixtures:
- pass: atm-bd reads bd ready, then atm task list, and skips the sanity bead because its task already exists.
- fail: atm-bd caches the task list from the previous tick and assigns the sanity bead a second time.

## ANC-062

Anchor: The dev template must tell the dev to run /sc-gh-stack and verify with /sc-gh-stack-view, but the close hook must accept a PR whose base equals the stack top when GraphQL is unavailable, so sanity and QA proceed; stack hygiene is the lead's, performed on the sanity PASS report.

> agreed.  we should 'tell the agent' to /sc-gh-stack and verify w/ /sc-gh-stack-view, but if graphql not working, aligning base w/ top is sufficient so sanity/qa proceed.  lead is responsible for stack hygiene.

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: retired. Version 3.

Summary: stack linking advised, base alignment sufficient, lead owns hygiene

Scope: dev-template.xml.j2 stacking step; the hook's PR base check; team-lead sanity PASS handling

Deterministic check to pair with the development gate: dev-template.xml.j2 names /sc-gh-stack and /sc-gh-stack-view; the hook passes a PR with base == pr_target and no stack link; team-lead requirement 22 links on sanity PASS

Fixtures:
- pass: The dev ran /sc-gh-stack-view, GraphQL failed, the PR base is the stack top: the hook closes and sanity is assigned; the lead links the layer after the PASS.
- fail: The hook refuses a PR that is not linked in a gh stack, or the dev is told to repair another layer before closing.

## ANC-063

Anchor: The sprint pour must write branch, worktree, pr_target and difficulty onto every generated dev, fix, sanity and qa bead, and validate the projected full phase graph, including rendered prospective beads, before any persistent write; missing values fail without changing the database.

> file a gh issue to make sure the necessary info lands in the poured bead AND is validated w/ pydantic model and validation python method for all beads in phase.

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: retired. Version 4.

Summary: pour validates projected phase graph before writes

Scope: formula producers and bead-groups projected pour; bead_schema.py and validate-plan phase validation

Deterministic check to pair with the development gate: Injected tests omit metadata from existing and prospective sc-compose records and assert zero mutating calls plus unchanged database state in normal and --validate mode.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the sprint pour is bj-2
- `atm-bj-1`: the sprint pour is bj-2

Fixtures:
- pass: bd show atm-bj-2.group-sanity shows branch, worktree, pr_target, difficulty; validate-plan --phase bj runs validate_phase() over all beads and fails on a missing key.
- pass: The pour validates all current and rendered prospective records first and makes no write when one is invalid.
- fail: Only the sprint container carries branch and worktree, and bead_schema.py validates sprint and sanity beads only.
- fail: The pour attaches prospective records and only then reports a missing metadata key.

## ANC-064

Anchor: atm-core must run a repository-configured hook on task close (and later on the other task transitions) that receives role and a custom JSON payload defined by the hook, keeps the task open when the hook returns failure, and logs the result; until it ships, atm-task-close.py in the skill stands in for it.

> Long term, I would like this to occur in a atm-task-close hook which we could tap into.  we should file this as an important gh issue on atm-core repo because I don't believe this hook exists today.  we should assign hooks for most of the atm task transitions.  that would allow us to implement different hooks in different orchestration patterns.  the atm-task-close hook would accept a 'custom' json set of fields which would be defined by the custom python (not atm).  atm would simply pass the json to the python function which would return a Result<T,E> and if the hook returned failure, the task would remain open.  the result would get logged.  The atm-task-close hook would ALSO be routed by 'role' or 'role' would be a mandatory input argument to the python script

Said by Rand, phase-bj planning 2026-10-18: Rand's close-hook and mechanical-assignment rulings (atm-core #1873 comment 1, atm-bd-orchestration #340, PR #342), 2026-10-18. Status: retired. Version 3.

Summary: atm task transition hooks in atm-core, routed by role, failure keeps the task open

Scope: atm-core task close path (crates/atm/src/commands/task.rs TaskCloseCommand) and its hook configuration; atm-core #1873, release 1.6.2

Deterministic check to pair with the development gate: with a configured hook that returns failure, atm task close <id> completed leaves the task active and the hook result appears in atm task events; with no hook configured the close behaves as today

Fixtures:
- pass: A repo-configured task-close hook returning err keeps the task active and logs the err; the hook receives role and the JSON payload unchanged.
- fail: atm closes the task before running the hook, or the hook's JSON fields are defined by atm.

## ANC-065

Anchor: On a sanity PASS the sanity side must link the checked PR as the new top of the phase stack before the close hook runs; when the checked head does not descend from the recorded top because another layer linked first, it must send the dev rebase-required on the dev's open task instead of linking, and the dev must rebase before every resubmission.

> what if we had dev rebase before sending to sanity, and sanity agent actually add to stack.  This would make handling 2 dev complete which were both based on the same <top-X> branch rast through sanity.  the first sanity agent determined PASS would get linked and sent to quality-mgr.  the other that completed sanity as PASS and could not be stacked w/out a rebase was sent back to dev to rebase and re-submit to sanity.  If sanity FAIL, the dev agent would be responsible for rebasing anyhow.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: sanity links the layer on PASS; first PASS wins the top, the other gets rebase-required

Scope: dev-sanity PASS procedure, sanity-stack, the sanity close hook's PASS precondition, templates/rebase-required.xml.j2; replaces ANC-062's lead-links clause

Deterministic check to pair with the development gate: two scratch branches cut from the same top both pass sanity: the first is linked and the stack bead's layers has it as top; the second gets one rebase-required send, no link, no ledger write; the hook refuses a PASS close whose vars carry no stack result

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the link on PASS and the rebase-required path are bj-3; bj-1 only refuses a PASS close whose vars carry no stack result and ships the rebase-required template
- `atm-bj-2`: the link, the sanity-stack subagent, the claim timeout and lean mode are bj-3; bj-2 ships the stack bead they write

Fixtures:
- pass: Sanity PASS on branch A links A as the top; sanity PASS on branch B, cut from the old top, sends the dev rebase-required and writes nothing; the dev rebases B onto A and runs its hook again.
- fail: The lead links the layer when it reads the sanity PASS report, or sanity closes the dev's task and assigns QA with the PR still unlinked.

## ANC-066

Anchor: dev-sanity must spawn the sanity-stack subagent for the link whenever more than one sanity task is open (or its own inline link failed), one spawn at a time in verdict order; each spawn handles exactly one next-top layer and the close procedure runs only after it returns stacked.

> we should create a sanity-stack subagent.  IF more than 1 sanity tasks are queued, sanity agent MUST use santiy-stack subagent to do the stacking and after successful stack run the close procedure.  the sanity-stack subagent should handle a single next-top layer.  multiple sanity complete sanity-stack agent spawns must be done sequentially.  This should keep things from falling apart when 10 sanity tasks land at the same time. ... dealing with the 'race condition' is the most important thing the agent does.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: sanity-stack subagent: one next-top layer per spawn, sequential; the race is its job

Scope: agents/sanity-stack.md, agents/dev-sanity.md PASS handling

Deterministic check to pair with the development gate: dev-sanity.md forbids a second sanity-stack spawn while one is running; sanity-stack.md takes one checked bead per input; ten simultaneous PASSes in a scratch run produce ten sequential link events in the stack bead history

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the sanity-stack subagent and dev-sanity's spawn rule are bj-3
- `atm-bj-2`: the link, the sanity-stack subagent, the claim timeout and lean mode are bj-3; bj-2 ships the stack bead they write
- `atm-bj-4`: the sanity-stack subagent and dev-sanity's spawn rule are bj-3

Fixtures:
- pass: With three sanity tasks open dev-sanity spawns sanity-stack for the first PASS, waits for stacked, then spawns it for the next; each spawn links one layer.
- fail: dev-sanity spawns one sanity-stack per PASS in parallel, or one spawn links several layers at once.

## ANC-067

Anchor: A stack bead under the phase epic must hold the entire stack (ordered layers with branch, PR, head, base, linked) as the only record of layer order; sanity-stack and the lead must claim it before any stack write and release it after; when GraphQL is back the subagent links every layer recorded unlinked; the bead closes when the stack lands.

> There needs to be local ledger the agent writes, which would be the phase bead if I understand properly.  entire stack would land in the phase bead.  |  One other responsibility of the subagent would be if graphql was out, but we can use not, it would stack ALL skipped layers.  |  On a side note, lead should probably coordinate w/ this agent as well when stacking work needs to be done.  this makes me think perhaps we need a stack-lock bead (child of phase epic).  stack-lock would be claimed before updating stack and returned to open when stack was complete.  (i believe using a bd as a lock is a known pattern.  stack-lock bead would represent a single stack, and would be closed when stack was merged.)

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: stack bead: the whole stack as ledger, its claim as the lock, lead coordinates through it

Scope: the stack bead (<prefix>-phase-<x>.stack) and every stack write in sanity-stack and the lead's stack recipes

Deterministic check to pair with the development gate: bd show <stack bead> --json after two links shows layers in link order; a link attempted without the claim is absent from the procedures; a lean-mode layer shows linked false and the next full-mode spawn links it first

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead

Fixtures:
- pass: sanity-stack claims atm-phase-bj.stack, appends the layer to metadata.layers, releases; the lead claims it before gh stack unstack; a layer recorded linked false is linked by the next full-mode spawn.
- fail: The top is kept on the phase root as a single branch name, or the lead unstacks without claiming the stack bead, or an unlinked layer stays unlinked after GraphQL returns.

## ANC-068

Anchor: sanity-stack must take a timeout for the stack bead claim, default 60 seconds, and on expiry must return the holder's identity, claim time and active task; dev-sanity must then keep the sanity task active, ask the holder once, continue other sanity work and retry; neither may refuse the task, fail the dev or escalate to a person.

> make sure there is a --timeout sent to the sanity-stack subagent for how long to wait if lock not available, sanity agent should also report who has the lock if --timeout occurs and lock could not be obtained.  This can not result in an team is blocked because agent could not get lock, likewise it can NOT result in escelate to HUMAN.  The team needs to know how to handle this and not disrupt work underway. ... 300 seconds and 15 seconds both seem quite long depending on how this is implemented.  can you attempt to claim w/ a 1-minute timeout for instance?

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 3.

Summary: lock timeout: --timeout input, report the holder, never blocks the team, never a human

Scope: sanity-stack input timeout_s (default 60, retry 5 s), its lock_timeout result, dev-sanity's handling, the stack bead lease

Deterministic check to pair with the development gate: a scratch run with the stack bead held by another actor: sanity-stack returns lock_timeout naming the holder within 60 s; dev-sanity's procedure has no refused close and no human escalation on that path; a claim older than lease_s is reclaimed by the subagent

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead
- `atm-bj-2`: the link, the sanity-stack subagent, the claim timeout and lean mode are bj-3; bj-2 ships the stack bead they write

Fixtures:
- pass: sanity-stack returns lock_timeout {holder: team-lead, since: ..., task: ...} after 60 s; dev-sanity notes STACK.LOCK_WAIT, messages team-lead once, runs its other sanity checks, retries.
- fail: dev-sanity closes the sanity task refused because the lock was busy, or sends the dev a FAIL, or sends the user a message asking who holds the lock.

## ANC-069

Anchor: sanity-stack must take a budget input in its fenced JSON; in lean mode, or whenever GraphQL is unavailable, it must verify only that the PR base equals the recorded top and that the head descends from it with no divergence, using one filtered REST pull list and local git, and must make no GraphQL call.

> yes, so basically if graphql is out, or we are watching our tokens (this should be an input arg to the fenced json input of the subagent) the subagent should make sure the branches are aligned and there is no divergence.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: lean mode: GraphQL out or token budget means alignment and no divergence only

Scope: sanity-stack input budget and its lean procedure

Deterministic check to pair with the development gate: with GH_TOKEN scoped to REST only, a lean spawn completes with zero graphql calls in the gh debug log and the layer recorded linked false; the REST call is filtered to number, base, head, sha, draft

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead
- `atm-bj-2`: the link, the sanity-stack subagent, the claim timeout and lean mode are bj-3; bj-2 ships the stack bead they write

Fixtures:
- pass: budget: lean: the subagent reads the recorded top, fetches, checks merge-base --is-ancestor, reads base.ref from the filtered REST list, appends the layer with linked false.
- fail: The subagent runs /sc-gh-stack-view or gh stack view --json in lean mode, or reads the raw 365 KB pull list into its context.

## ANC-070

Anchor: The qa role of the close hook must close the QA bead and task and send the phase lead a status message in both the PASS and the FAIL case, and must assign nothing; quality-mgr pours the fix groups before the close as today.

> yes, quality atm-task-close hook today would simply inform <phase-lead> w/ a status message in both pass or fail case.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: qa hook informs the lead only, PASS or FAIL

Scope: atm-task-close.py qa role

Deterministic check to pair with the development gate: a qa-role run in a scratch repo with verdict FAIL closes the bead and task, sends exactly one message to the lead, and issues no atm task assign

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the close hook's roles, checks, round counter and messages are bj-1; bj-2 ships the stack bead schema and the pour only
- `atm-bj-3`: the hook's roles, identity, checks, FAIL counter and messages are bj-1; bj-3's procedures end by running that hook

Fixtures:
- pass: The qa role closes atm-bj-2.group-qa and its task and sends the lead the qa status; the poured fix beads wait in bd ready for the lead.
- fail: The qa role assigns the poured fix beads to devs, or sends the lead nothing on PASS.

## ANC-071

Anchor: A rebase-required outcome must not increment metadata.sanity_round on the sanity bead or count toward max_sanity_rounds.

> max_sanity_rounds does NOT increment on rebase-required.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: max_sanity_rounds does not move on rebase-required

Scope: the rebase-required path of dev-sanity and sanity-stack

Deterministic check to pair with the development gate: after a rebase-required and a later PASS in a scratch run, sanity_round on the sanity bead is unchanged from before

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: BJ-2 ships stack schema, pour and validation only; the rebase-required sanity path is owned by BJ-1 and BJ-3.

Fixtures:
- pass: Branch B loses the top twice and then passes; sanity_round stays 0.
- fail: sanity_round is 2 after two rebase-required outcomes.

## ANC-072

Anchor: A sanity FAIL must be delivered only as the sanity-fail.xml.j2 message to the dev on the open task, listing exactly the deliverables that were NOT complete with the reviewer's reason and referencing the sprint; no finding bead, child bead or lead action may be created for it.

> sanity FAIL would be completely handled by ATM.  the sanity-fail.xml.j2 template would list exactly what was NOT complete and reference the sprint.

Said by Rand, phase-bj planning 2026-10-18 (second round): Rand's sanity-stack, stack bead and lock rulings (atm-bd-orchestration PR #342, docs/requirements/sanity-stack.md), 2026-10-18. Status: locked. Version 2.

Summary: sanity FAIL handled entirely by ATM: sanity-fail.xml.j2 lists exactly what was NOT complete and references the sprint

Scope: templates/sanity-fail.xml.j2 and the sanity role's FAIL branch; retirement of sanity-create-findings

Deterministic check to pair with the development gate: a FAIL run in a scratch repo produces one sanity-fail message whose body names the sprint id and each undone deliverable number, and bd list --parent <checked bead> stays empty

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: the close hook's roles, checks, round counter and messages are bj-1; bj-2 ships the stack bead schema and the pour only
- `atm-bj-3`: the sanity-fail message and the hook's FAIL branch are bj-1
- `atm-bj-4`: the sanity FAIL message and branch are bj-1; bj-4's tabs only read QA run and finding data already on beads

Fixtures:
- pass: The dev receives sanity-fail: sprint bj-2, deliverables 2 and 5 NOT complete with reasons; no beads are created.
- fail: The hook files a finding bead per undone deliverable under the checked bead, or the lead is asked to dispatch a fix.

## ANC-073

Anchor: atm-bd must show a sanity runs page and a quality runs page, newest run on top, one line per run; the sanity line must carry a pass/fail column and an elapsed-time column for JEV and for LLM each, and a column with the name and nerd icon of the dev whose work was checked; elapsed comes from the Jev client's own return and from a timer fenced around the LLM subagent, and the same rows feed the OTel database in the next atm release.

> sanity agent never does it and I always need to ask.  it is the only thing I want to see from sanity agent.  perhaps we should consider adding a tui page for both sanity and quality which showed the runs sorted by most recent (on top).  the sanity page would have 1 line per run and both jev + llm would have columns for pass/fail AND elapsed time (which is currently not calculated properly).  Regarding calculation, can't this be returned from the jev.py method call and a timer fencing around the llm agent?  I would also like to have a column for what dev (name and nerd icon) did the work under review.  This would remove the agent distraction and show the data I want to see (and get into otel db on next atm release).

Said by Rand, phase-bj planning 2026-10-18 (third round): Rand on the sanity runs table, Jev/LLM failures and refusal ceremony, 2026-10-18. Status: locked. Version 2.

Summary: atm-bd runs pages for sanity and quality: one line per run, newest first, JEV and LLM verdict and elapsed, dev name and icon

Scope: atm-bd console pages, the sanity ledger row schema (elapsed per reviewer, dev identity), jev_client.py return value, the dev-sanity LLM spawn

Deterministic check to pair with the development gate: the console has a sanity page and a quality page; a ledger row has jev_elapsed_s, llm_elapsed_s and dev fields; jev_client.py returns elapsed; the dev-sanity prompt wraps the LLM spawn in a start/end time; the sanity reply template no longer renders the runs table

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the runs pages and the ledger row schema are bj-4; bj-3 adds only the LLM timer and retry wording in dev-sanity.md whose values bj-4's rows carry
- `atm-bj-2`: the runs pages and the sanity ledger are bj-4; bj-2 ships the stack bead schema and the pour only

Fixtures:
- pass: The sanity page lists runs newest first: bj-2 #1904 dev solar (icon) JEV ✅ 41 s LLM ✅ 88 s; dev-sanity's reply carries only the verdict.
- fail: The runs table is rendered into dev-sanity's chat reply, or elapsed is the gap between two ledger timestamps, or the dev column is missing.

## ANC-074

Anchor: A Jev API call failure gets up to three retries (four total attempts) at the Jev client boundary before the slot reports failure; its reason must appear in the runs page severity table with an icon distinct from a JEV FAIL verdict; no workflow class bead, outage bead, hold on re-dispatch, refusal citation or checker-defect side channel may exist in the sanity or refusal flows.

> this entire process of hold on re-dispatch sounds like a team trying to manage a reliable asset by making excuses not to deal with failures to call the agent properly.  (I suspect that a subagent sends too much context and jev returns error, instead of editing context, failure is returned.  There are occasionally LLM call fails during heavy traffic times which can be retried 3x before saying fail. )  Make sure that LLM failure reason shows up in severity table in tui AND the fail icon for typesafe API issue is distinctly different from jev answered 'all work NOT done'.  this `Class-bead append and citation on every refusal, and the checker-defect side channel.` also sounds like ceremony I did not know about or approve.

Said by Rand, phase-bj planning 2026-10-18 (third round): Rand on the sanity runs table, Jev/LLM failures and refusal ceremony, 2026-10-18. Status: retired. Version 5.

Summary: lead ruling 2026-10-18: Jev client owns three retries, four attempts total

Scope: jev_client.py single retry boundary; ledger kind; console severity icon; sanity/refusal flows; the LLM coordinator adds no second retry loop

Deterministic check to pair with the development gate: Injected client tests assert up to four total attempts for transport/502/rate-limit failures with controlled clock, oversized-context no-retry, elapsed and API_FAILURE reason; console maps API_FAILURE to a distinct icon.

Fixtures:
- pass: A Jev 502 is retried three times, then the row reads kind API_FAILURE with the error text and the page shows the plug icon, not the red cross of a FAIL verdict; dev-sanity re-runs on the next task with no bead opened.
- pass: The Jev client retries a 502 under its bounded policy, then records API_FAILURE and the console shows its distinct icon and error.
- fail: dev-sanity opens a -jev-outage class bead and the lead holds re-dispatch until it closes, or an API failure is shown with the same icon as a JEV FAIL.
- fail: The client immediately fails a retryable 502, or the API_FAILURE row uses the JEV FAIL icon.

## ANC-075

Anchor: The package publish script must write a release-artifact manifest with one sc-sha path/hash entry for every shipped agent, skill, Python file, template, and script, plus the aggregate.

> a package script inside the skill package computes the sc-sha of every shipped file (agents, skills, python, templates, scripts) and writes a manifest (per-file path + sha, plus the aggregate) into the release artifact.

Said by Rand, atm-bd-orchestration issue #349 scope, 2026-10-18. Status: retired. Version 3.

Summary: Publish full-skill sc-sha manifest

Scope: edits the package publish script, release artifact contents, or packaged agents, skills, Python files, templates, or scripts

Deterministic check to pair with the development gate: Inspect the package publish script and produced release archive; verify each shipped file has one path/hash manifest entry and the aggregate matches those entries.

Fixtures:
- pass: The package publish script puts path/hash manifest entries for every shipped agent, skill, Python file, template, and script in the release artifact and includes the aggregate.
- fail: The package publish script includes only Python files in the release manifest and omits the aggregate hash.

## ANC-076

Anchor: The atm-bd skill installer must verify every installed file against the manifest published for the selected tag or --latest version.

> `atm-bd skill` verifies every file against the published manifest for the version it installs (tag or `--latest`) and refuses a mismatch with the file list.

Said by Rand, atm-bd-orchestration issue #349 scope, 2026-10-18. Status: retired. Version 3.

Summary: Verify installed skill files at install

Scope: edits the atm-bd skill installer or its version selection and file verification

Deterministic check to pair with the development gate: Run installer fixtures for matching, modified, and missing files at a tag and --latest; verify mismatches stop installation and list each affected path.

Fixtures:
- pass: The atm-bd skill installer checks each installed file against the published manifest for the selected --latest release.
- fail: The atm-bd skill installer accepts a release with a modified installed script without checking the published manifest.

## ANC-077

Anchor: The plugin doctor must report each installed file as ok, modified, missing, or extra.

> reports per-file ok|modified|missing|extra

Said by Rand, atm-bd-orchestration issue #349 scope, 2026-10-18. Status: retired. Version 3.

Summary: Report installed file integrity status

Scope: edits the plugin doctor or its installed-file integrity reporting

Deterministic check to pair with the development gate: Run atm-hook-doctor.py against fixture installs containing unchanged, modified, missing, and extra files; assert one status per path.

Fixtures:
- pass: The plugin doctor reports every installed file as ok, modified, missing, or extra.
- fail: The plugin doctor reports only aggregate PASS or FAIL and gives no status for individual installed files.

## ANC-078

Anchor: The branch value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.

> when the sprint is poured, wouldn't the branch/worktree land in the dev (or fix), sanity and qa bead at that point?
078-081 this is the formula we need to pour (gap in current implementation) when dev/qa/sanity are poured (so all get the same info in bead).

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 4.

Summary: Sprint-family beads carry branch

Scope: atm-bd-orchestration sprint-family pour

Deterministic check to pair with the development gate: Inspect rendered sprint-family beads and verify each dev, fix, sanity and QA bead has the expected branch value.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the pour and its sprint-family metadata are bj-2; bj-1's close hook only reads the fields the pour wrote
- `atm-bj-4`: the pour and its sprint-family metadata are bj-2; bj-4's dispatch only reads the fields the pour wrote
- `atm-bj-3`: the pour, its metadata and the bead models are bj-2; bj-3 is the sanity-stack agent and dev-sanity's PASS path

Fixtures:
- pass: The pour copies the sprint container's branch value onto every generated sprint-family dev, fix, sanity and QA bead.
- fail: The sprint-family pour writes no branch value onto its generated dev, fix, sanity or QA beads.

## ANC-079

Anchor: The worktree value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.

> when the sprint is poured, wouldn't the branch/worktree land in the dev (or fix), sanity and qa bead at that point?
078-081 this is the formula we need to pour (gap in current implementation) when dev/qa/sanity are poured (so all get the same info in bead).

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 4.

Summary: Sprint-family beads carry worktree

Scope: atm-bd-orchestration sprint-family pour

Deterministic check to pair with the development gate: Inspect rendered sprint-family beads and verify each dev, fix, sanity and QA bead has the expected worktree value.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the pour and its sprint-family metadata are bj-2; bj-1's close hook only reads the fields the pour wrote
- `atm-bj-4`: the pour and its sprint-family metadata are bj-2; bj-4's dispatch only reads the fields the pour wrote
- `atm-bj-3`: the pour, its metadata and the bead models are bj-2; bj-3 is the sanity-stack agent and dev-sanity's PASS path

Fixtures:
- pass: The pour copies the sprint container's worktree value onto every generated sprint-family dev, fix, sanity and QA bead.
- fail: The sprint-family pour writes no worktree value onto its generated dev, fix, sanity or QA beads.

## ANC-080

Anchor: The pr_target value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.

> branch, worktree, pr_target, difficulty land on the sprint container (as today) AND on its sanity and QA beads at pour time, copied from the container.
078-081 this is the formula we need to pour (gap in current implementation) when dev/qa/sanity are poured (so all get the same info in bead).

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 4.

Summary: Sprint-family beads carry pr_target

Scope: atm-bd-orchestration sprint-family pour

Deterministic check to pair with the development gate: Inspect rendered sprint-family beads and verify each dev, fix, sanity and QA bead has the expected pr_target value.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the pour and its sprint-family metadata are bj-2; bj-1's close hook only reads the fields the pour wrote
- `atm-bj-4`: the pour and its sprint-family metadata are bj-2; bj-4's dispatch only reads the fields the pour wrote
- `atm-bj-3`: the pour, its metadata and the bead models are bj-2; bj-3 is the sanity-stack agent and dev-sanity's PASS path

Fixtures:
- pass: The pour copies the sprint container's pr_target value onto every generated sprint-family dev, fix, sanity and QA bead.
- fail: The sprint-family pour writes no pr_target value onto its generated dev, fix, sanity or QA beads.

## ANC-081

Anchor: The difficulty value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.

> branch, worktree, pr_target, difficulty land on the sprint container (as today) AND on its sanity and QA beads at pour time, copied from the container.
078-081 this is the formula we need to pour (gap in current implementation) when dev/qa/sanity are poured (so all get the same info in bead).

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 4.

Summary: Sprint-family beads carry difficulty

Scope: atm-bd-orchestration sprint-family pour

Deterministic check to pair with the development gate: Inspect rendered sprint-family beads and verify each dev, fix, sanity and QA bead has the expected difficulty value.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: the pour and its sprint-family metadata are bj-2; bj-1's close hook only reads the fields the pour wrote
- `atm-bj-4`: the pour and its sprint-family metadata are bj-2; bj-4's dispatch only reads the fields the pour wrote
- `atm-bj-3`: the pour, its metadata and the bead models are bj-2; bj-3 is the sanity-stack agent and dev-sanity's PASS path

Fixtures:
- pass: The pour copies the sprint container's difficulty value onto every generated sprint-family dev, fix, sanity and QA bead.
- fail: The sprint-family pour writes no difficulty value onto its generated dev, fix, sanity or QA beads.

## ANC-082

Anchor: validate-plan must validate every bead kind in the phase against a Pydantic model that names its required metadata keys.

> A pydantic model for every bead kind in a phase, not just sprint and sanity: dev/sprint container, sanity, QA (with checked_bead and the round), gate, finding, fix, and their sanity/QA children. Each model names the required metadata keys; extra="allow" stays.
we need model for every bead kind verified at validation.

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 5.

Summary: Every bead kind has a typed model

Scope: atm-bd-orchestration bead schema

Deterministic check to pair with the development gate: Validate fixtures for every phase bead kind and assert the selected Pydantic model enforces its required metadata keys.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: the pour, its metadata and the bead models are bj-2; bj-3 is the sanity-stack agent and dev-sanity's PASS path

Fixtures:
- pass: validate-plan checks every bead kind in the phase (sprint container, dev, fix, sanity, QA, gate, finding) against its Pydantic model and rejects a bead missing a required metadata key.
- fail: validate-plan checks only sprint and sanity beads; a finding bead with no model passes unchecked.

## ANC-083

Anchor: A single validation method must validate every bead in the phase.

> One validation method that covers all beads in the phase.

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 3.

Summary: Validate every bead in the phase

Scope: atm-bd-orchestration bead schema validation

Deterministic check to pair with the development gate: Run the phase validator over a fixture phase and confirm it visits every bead under the phase epic.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: The phase validator belongs to bj-2, not the bj-1 task-close hook.
- `atm-bj-3`: The phase validator and bead_schema.py belong to bj-2, not bj-3 sanity-stack.
- `atm-bj-4`: Phase validation is bj-2; bj-4 does not change it.

Fixtures:
- pass: The phase validation method visits every bead under the phase epic.
- fail: The phase validation method skips a bead under the phase epic.

## ANC-084

Anchor: A Jev call that returns an API-busy error must be retried up to three times before the API-busy failure is reported.

> There are occasionally LLM call fails during heavy traffic times which can be retried 3x before saying fail.
most for jev, if API busy returned, retry before giving up/returning API-busy ( I see this at peak times, a couple retries one generally works).

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 8.

Summary: Retry LLM call failures three times

Scope: the Jev client used by dev-sanity and its sc-sanity-jev subagent

Deterministic check to pair with the development gate: a controlled call that answers API-busy twice then succeeds yields the result; four API-busy answers report API-busy after the third retry

Fixtures:
- pass: A Jev call that gets API busy is retried; after three busy retries the slot reports API-busy.
- fail: The first API-busy answer from Jev is reported as the sanity result with no retry.

## ANC-085

Anchor: The runs page severity table must show the LLM failure reason and use an icon distinct from a JEV not-done verdict for a TypeSafe API failure.

> Make sure that LLM failure reason shows up in severity table in tui AND the fail icon for typesafe API issue is distinctly different from jev answered 'all work NOT done'.

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: retired. Version 3.

Summary: Show API failure reason and distinct icon

Scope: atm-bd-orchestration runs page severity table

Deterministic check to pair with the development gate: Render an API failure and a JEV not-done verdict and assert the reason is visible and their icons differ.

Fixtures:
- pass: The runs page severity table shows the LLM failure reason and gives TypeSafe API failure a distinct icon from a JEV not-done verdict.
- fail: The runs page severity table omits the LLM failure reason and uses the JEV not-done icon for a TypeSafe API failure.

## ANC-086

Anchor: A sanity FAIL or refusal must be sent only as an ATM message, with no outage class bead and no refusal citation.

> This `Class-bead append and citation on every refusal, and the checker-defect side channel.` also sounds like ceremony I did not know about or approve.
yes sanity fail/refuse are just atm message to dev now.

Said by Rand, phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, 2026-10-18. Status: locked. Version 5.

Summary: No outage beads or refusal citations

Scope: atm-bd-orchestration sanity and refusal flows

Deterministic check to pair with the development gate: the sanity FAIL and refusal paths send one ATM message to the dev and create no bead; no refusal path requires a citation

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-2`: bj-2 ships the stack bead schema, the pour metadata and validate-plan; it has no sanity FAIL or refusal path
- `atm-bj-4`: bj-4 renders the runs tabs and owns the Jev client; the sanity FAIL and refusal paths are bj-1's hook and bj-3's dev-sanity text

Fixtures:
- pass: On FAIL dev-sanity sends the dev one ATM message; a refusal closes the task refused; neither creates a bead or cites a source.
- fail: A sanity refusal opens an outage class bead and must cite the refusal reason's source.

## ANC-087

Anchor: The published skill must include a manifest containing the hash of every shipped file.

> when atm-bd is published, the sha must be calculated and stored in a manifest for every file. then the installer can check against the published manifest to verify the file has not changed. we should do this for the entire skill. all agents/skills/python files.

Said by Rand, atm-bd-orchestration issue #349, Rand verbatim, 2026-10-18. Status: locked. Version 2.

Summary: Manifest hashes every published skill file

Scope: atm-bd skill publishing

Deterministic check to pair with the development gate: Inspect a published skill manifest and verify each shipped file has a hash.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-3`: Skill publication and its manifest belong to bj-5, not bj-3 sanity-stack.
- `atm-bj-4`: The skill manifest belongs to bj-5 and is not touched by bj-4.

Fixtures:
- pass: The published skill has a manifest listing the hash of each shipped file.
- fail: The published skill manifest omits the hash for a shipped file.

## ANC-088

Anchor: The atm-bd installer must verify skill files against the published manifest.

> when atm-bd is published, the sha must be calculated and stored in a manifest for every file. then the installer can check against the published manifest to verify the file has not changed. we should do this for the entire skill. all agents/skills/python files.

Said by Rand, atm-bd-orchestration issue #349, Rand verbatim, 2026-10-18. Status: locked. Version 2.

Summary: Installer verifies skill files against manifest

Scope: atm-bd skill installer

Deterministic check to pair with the development gate: Install a skill with a changed file and verify it is detected against the published manifest.

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bj-1`: Installer verification belongs to bj-5, not the bj-1 task-close hook.
- `atm-bj-3`: Installer verification belongs to bj-5, not bj-3 sanity-stack.
- `atm-bj-2`: Installer verification belongs to bj-5, not the bj-2 pour and validate-plan.

Fixtures:
- pass: The installer verifies skill files against the published manifest.
- fail: The installer accepts skill files without comparing them to the published manifest.

## ANC-089

Anchor: The vars file given to atm-task-close.py must contain the union of the required variables of every template it is passed to, atm task templates and sc-compose bead templates alike, so one vars file serves every render.

> same vars sent to 'atm ... --template --vars <shared-vars>'  and sc-compose bead ... --template --vars <shared-vars>
yes, json should be union of required variables for both templates.

Said by Rand, phase-bj drift review 2026-10-18 (fenix relay, message 01M58YBB6RRSSCBMVM991MMAA3): Rand's answer on ANC-058, 2026-10-18. Status: locked. Version 3.

Summary: one vars file = union of atm task and sc-compose bead template variables

Scope: atm-task-close.py vars contract, task-close-vars.j2, the atm task and sc-compose bead templates the hook flow renders

Deterministic check to pair with the development gate: strict-render dev-sanity-bead.json.j2, qa-bead.json.j2 and every role's atm template with the one test vars file; each render passes and task-close-vars.j2 required_variables is the union of their headers

Fixtures:
- pass: task-close-vars.j2 lists every required variable of dev-sanity-template.xml.j2, qa-template.xml.j2, dev-sanity-bead.json.j2 and qa-bead.json.j2, and the hook passes the same file as --vars to atm and to sc-compose.
- fail: The hook keeps one vars file for the atm templates and builds a separate vars dict for the sc-compose bead templates.

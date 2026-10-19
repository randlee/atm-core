## Anchor gate: plan · atm-bj-1 - bj-1: close hook: atm-task-close.py roles dev, fix, sanity, qa; task-close-vars.j2, sanity-fail.xml.j2, rebase-required.xml.j2; max_sanity_rounds

Run `plan-20261019T003534Z-atm-bj-1`, model `jev-1.13.0`, threshold 0.6. Counts: underspecified 2, satisfied 12, not_applicable 36, deterministic_only 3, excluded 14.

> **atm-bj-1 · ANC-083 · underspecified** Anchor: A single validation method must validate every bead in the phase. You said (phase-bj planning 2026-10-18: Rand rulings, atm-bd-orchestration #340 and #347, Oct 18): "One validation method that covers all beads in the phase." Jev was not confident (min confidence 0.59); the anchor or the bead may be underspecified. (leaning: not_addressed)
>   Low: relevance `relevant` answered yes at 0.59 (second ask).
>   Unstable: two identical requests gave not_addressed then underspecified.

> **atm-bj-1 · ANC-088 · underspecified** Anchor: The atm-bd installer must verify skill files against the published manifest. You said (atm-bd-orchestration issue #349, Rand verbatim, Oct 18): "when atm-bd is published, the sha must be calculated and stored in a manifest for every file. then the installer can check against the published manifest to verify the file has not changed. we should do this for the entire skill. all agents/skills/python files." Jev was not confident (min confidence 0.40); the anchor or the bead may be underspecified. (leaning: not_applicable)
>   Low: relevance `relevant` answered no at 0.40.

> **atm-bj-1 · ANC-005 · excluded** Anchor: No sprint in this phase must change what ships in v0.16.1. Not judged: this sprint lands on integrate/phase-bj after the v0.16.1 tag and changes nothing that v0.16.1 ships.

> **atm-bj-1 · ANC-028 · excluded** Anchor: The console must show bead open with U+F0766 in #2D81E0, sprint not_ready with U+F137C in #D97757, PR open with U+F04C2 in #2D81E0, PR draft with U+F0CB6 in #FFA60D and CI none with U+F01D8 in #2D81E0. Not judged: bj-1 edits the close hook script and its templates and renders no bead, sprint, PR or CI state in the console; ANC-028 scope is console rendering (relevance no at 0.56 and 0.79, leaning not_applicable, same applicability-noise class as the other phase-bi exclusions).

> **atm-bj-1 · ANC-033 · excluded** Anchor: The status table's pr cell must link the PR number to the PR url GitHub returned for that PR (PullRequest.url); it must not derive the link from a base URL or call gh pr view per PR. Not judged: BJ-1 does not own console PR status-table rendering; it changes only the close hook and its templates.

> **atm-bj-1 · ANC-042 · excluded** Anchor: atm-bd sends every ATM message as the constant address atm-bd@tool; the only override is an optional <identity>@<team> key in the repository's .claude/project/atm-bd-orchestration.yaml. Not judged: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's.

> **atm-bj-1 · ANC-043 · excluded** Anchor: atm-bd must not take its sender address from ATM_IDENTITY or ATM_TEAM; ATM_TEAM may only select the team the tool executes against. Not judged: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's.

> **atm-bj-1 · ANC-046 · excluded** Anchor: A failed command is logged at level error with the message <fix in backticks> <command words> [<exit code>] <first stderr line>, composed once by the writer; the command words are the executable and subcommand, not the full argument vector. Not judged: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's.

> **atm-bj-1 · ANC-047 · excluded** Anchor: A failure is logged once when it happens; a cached failure still on display (such as the last GitHub error kept with cached PR data) is a sheet status, never a new log entry per tick. Not judged: BJ-1 does not own cached console error logging or PR display; its scope is the close hook.

> **atm-bj-1 · ANC-049 · excluded** Anchor: Before each GitHub refresh atm-bd reads the live remaining quota for the resource its mode uses (graphql or core) via gh api rate_limit; when remaining is below the configured threshold (default 50 percent of the limit) it skips the refresh, keeps cached PR data, shows a paused status with the numbers and the local reset time, and refuses forced refreshes until the quota is back above the threshold. Not judged: atm-bd's sender address, log format and GitHub quota read are atm-bd tool code that bj-1 does not edit; the hook is a skill script whose identities are ANC-051's.

> **atm-bj-1 · ANC-061 · excluded** Anchor: atm-bd must assign a sanity or qa bead only when bd ready lists it and, checked afterwards, no ATM task with that bead id exists; a bead whose task the hook already assigned is never assigned a second time. Not judged: bj-1 does not touch atm-bd dispatch; the read order ANC-061 names is bj-4's deliverable 8.

> **atm-bj-1 · ANC-065 · excluded** Anchor: On a sanity PASS the sanity side must link the checked PR as the new top of the phase stack before the close hook runs; when the checked head does not descend from the recorded top because another layer linked first, it must send the dev rebase-required on the dev's open task instead of linking, and the dev must rebase before every resubmission. Not judged: the link on PASS and the rebase-required path are bj-3; bj-1 only refuses a PASS close whose vars carry no stack result and ships the rebase-required template.

> **atm-bj-1 · ANC-066 · excluded** Anchor: dev-sanity must spawn the sanity-stack subagent for the link whenever more than one sanity task is open (or its own inline link failed), one spawn at a time in verdict order; each spawn handles exactly one next-top layer and the close procedure runs only after it returns stacked. Not judged: the sanity-stack subagent and dev-sanity's spawn rule are bj-3.

> **atm-bj-1 · ANC-067 · excluded** Anchor: A stack bead under the phase epic must hold the entire stack (ordered layers with branch, PR, head, base, linked) as the only record of layer order; sanity-stack and the lead must claim it before any stack write and release it after; when GraphQL is back the subagent links every layer recorded unlinked; the bead closes when the stack lands. Not judged: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead.

> **atm-bj-1 · ANC-068 · excluded** Anchor: sanity-stack must take a timeout for the stack bead claim, and on expiry must return the holder's identity, claim time and active task; dev-sanity must then keep the sanity task active, ask the holder once, continue other sanity work and retry; neither may refuse the task, fail the dev or escalate to a person. Not judged: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead.

> **atm-bj-1 · ANC-069 · excluded** Anchor: sanity-stack must take a budget input in its fenced JSON; in lean mode, or whenever GraphQL is unavailable, it must verify only that the PR base equals the recorded top and that the head descends from it with no divergence, using one filtered REST pull list and local git, and must make no GraphQL call. Not judged: the stack bead writes, the claim timeout and lean mode are bj-2 and bj-3; the hook reads no stack bead.

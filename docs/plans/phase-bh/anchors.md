# Anchors - phase-bh

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bh/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | No logic chains | 2026-10-14 |
| ANC-002 | locked | Business logic in one place | 2026-10-14 |
| ANC-003 | locked | Same behavior cron or TUI | 2026-10-14 |
| ANC-004 | locked | Holds: production reasons plus merge conflict | 2026-10-14 |
| ANC-005 | locked | No bd list --all | 2026-10-14 |
| ANC-006 | locked | One GitHub call per cycle | 2026-10-14 |
| ANC-007 | locked | --as mandatory | 2026-10-14 |
| ANC-008 | locked | Per-agent queue shown | 2026-10-14 |
| ANC-009 | locked | Non-sprint tasks marked | 2026-10-14 |
| ANC-010 | locked | Escalation cadence | 2026-10-14 |
| ANC-011 | locked | Phase end or 60-min idle stops polling | 2026-10-14 |
| ANC-012 | locked | No line wrap | 2026-10-14 |
| ANC-013 | locked | No unnecessary text | 2026-10-14 |
| ANC-014 | locked | App never fails after launch | 2026-10-14 |
| ANC-015 | locked | Merge check each cycle | 2026-10-14 |
| ANC-016 | locked | Report red stack top only | 2026-10-14 |
| ANC-017 | locked | Work proceeds without GitHub | 2026-10-14 |
| ANC-018 | locked | GitHub off switches | 2026-10-14 |
| ANC-019 | locked | Status bar metrics | 2026-10-14 |
| ANC-020 | locked | Local 24h on screen, UTC stored | 2026-10-14 |
| ANC-021 | locked | Agent status shown, not messaged | 2026-10-14 |
| ANC-022 | locked | TUI data is JSON | 2026-10-14 |

## ANC-001

Anchor: Each atm-bd-console dispatch, hold, marker or section rule must be a single check on snapshot fields (one condition, possibly over several fields) that does not depend on the result of any other rule.

> avoid complicated logic chains.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 4.

Summary: No logic chains

Scope: any bead that defines or changes a dispatch, hold, escalation, marker or section rule

Fixtures:
- pass: Each dispatch and hold rule is one independent check on snapshot fields: the bead is in bd ready, its branch is on origin, its worktree exists, its merge state is clean; no rule reads another rule's result.
- fail: QA is dispatched only if sanity passed, and if that check fails the dispatcher then checks CI before deciding whether to retry the stack-top check.

## ANC-002

Anchor: Every atm-bd-console dispatch, escalation and section rule must be implemented in exactly one module, and the app, the cron mode and bead-queues must call that module rather than contain their own copy of any rule.

> business logic defined clearly in a single place.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Business logic in one place

Scope: any bead that adds code to render.py, cli.py, app.py or bead-queues

Fixtures:
- pass: All dispatch and section rules live in dispatch.py and model.py; render.py, cli.py and bead-queues only call them.
- fail: bead-queues keeps its own copy of the section rules so it can run without the app installed.

## ANC-003

Anchor: Running atm-bd-console as the cron job and as the TUI app must produce the same dispatches, messages and state for the same beads and tasks.

> identical behavior when run as cron or tui

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Same behavior cron or TUI

Scope: any bead that changes the tick, the app loop or the cron mode

Fixtures:
- pass: The app loop and --once --headless call the same tick function and a test runs both against one fixture and compares dispatches, messages and state.
- fail: The cron mode skips escalation batching to stay fast; only the app batches resends.

## ANC-004

Anchor: The atm-bd-console dispatcher must hold a sanity or QA dispatch only when the bead is not in bd ready, its branch is not on origin, its worktree is missing, or its branch has a merge conflict with its PR target; PR and CI state never hold it.

> we are working off a set of rules already in production.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 4.

Summary: Holds: production reasons plus merge conflict

Scope: any bead that defines when sanity or QA is dispatched or held

Fixtures:
- pass: The dispatcher sends sanity when bd ready lists the bead, the branch is on origin, the worktree exists and merge-tree is clean; a missing PR only adds a reminder.
- fail: QA waits until the PR's CI is green and the layer is the top of the stack.

## ANC-005

Anchor: atm-bd-console must never run `bd list --all`.

> Do NOT use `bd list --all`

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: No bd list --all

Scope: any bead that adds a bd read

Fixtures:
- pass: Each tick reads open beads with bd list -l phase-bh and fetches needed closed ids with one batched bd show.
- fail: Each tick runs bd list --all to find closed predecessors.

## ANC-006

Anchor: atm-bd-console must make at most one GitHub call per 10-minute cycle (the open-PR list), plus one each time the PRs tab opens or is refreshed.

> I thought there were no gh calls

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 4.

Summary: One GitHub call per cycle

Scope: any bead that adds a gh or GitHub API call

Fixtures:
- pass: One GraphQL query per 10-minute cycle returns every open PR with its branches and CI rollup; the PRs tab makes one more call on open and on r.
- fail: Each tick calls GitHub once per held bead to see whether its PR exists.

## ANC-007

Anchor: atm-bd-console must refuse to start without `--as` and must print a message explaining what `--as` is.

> I would prefer that bead-dispatch makes --as a mandatory arg w/ good console message if it is not set

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: --as mandatory

Scope: any bead that defines the CLI arguments or the assigning identity

Fixtures:
- pass: Without --as the CLI exits 3 and prints what --as is and an example command.
- fail: Without --as the CLI uses $ATM_IDENTITY as the lead.

## ANC-008

Anchor: The dashboard must show, for each agent, its total number of assigned tasks and at most 3 of those tasks.

> what i want to see is what tasks agents are assigned. i.e. queue per agent (up to a limit around ~3. if > 3 tasks, I still want to know queue depth).

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Per-agent queue shown

Scope: any bead that defines the agent blocks

Fixtures:
- pass: Each agent block shows the agent's queue depth and up to 3 task rows, then '… +N more'.
- fail: The dashboard lists only the running task of each agent.

## ANC-009

Anchor: The dashboard must mark each assigned task whose bead has no top epic or whose top epic is not the current phase root.

> I also want to know if these are not sprint tasks.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 4.

Summary: Non-sprint tasks marked

Scope: any bead that defines the snapshot rows or the agent blocks

Fixtures:
- pass: Every task row is checked against the phase: a task whose bead has no top epic, or whose top epic is not the phase root, shows a red marker; a task with no bead at all shows a yellow marker.
- fail: Tasks are listed by agent without any phase check.

## ANC-010

Anchor: Each new problem must be sent to its recipient (the lead, or the assignee for a merge conflict or a missing PR) in its own message when it is first seen; per recipient, problems still unresolved must be resent together in one message 10 minutes after that recipient's first send of the cycle and every 10 minutes after that.

> one message per problem, then resent after 10 minutes. if more issues show up, batch them into same 10 minute interval after 1st send.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 5.

Summary: Escalation cadence

Scope: any bead that defines escalation or hold messages

Fixtures:
- pass: Problems first seen at 1m, 2m and 3m are each sent to their recipient in their own message at once; the lead's unresolved ones are resent together at 11m and 21m, and an assignee's at 10 minutes after its own first send.
- fail: Each unresolved problem is resent individually every tick until resolved.

## ANC-011

Anchor: atm-bd-console must stop polling, dispatching and sending once the phase root bead is closed, or once the phase has had no ready and no in-progress bead for 60 minutes, whether it runs as the app or as the cron job; after the root check it makes no further reads.

> app would stop polling and cron would do nothing when phase ended.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 5.

Summary: Phase end or 60-min idle stops polling

Scope: any bead that defines the tick start, the app loop or the cron mode

Fixtures:
- pass: Root closed: the cron run exits 0 after the root check; 61 minutes with nothing ready or in progress: the app shows stopped: idle 60 min and stops ticking until r.
- pass: The idle stop counts only time with no ready and no in-progress bead, so a phase with one bead in progress for 2 hours keeps ticking, and it stops 60 minutes after that bead closes if nothing else is ready.
- fail: When the root closes, the app keeps polling every 60 s and shows no new rows.

## ANC-012

Anchor: Every dashboard line must fit the terminal width without wrapping, with descriptions truncated to fit.

> for all descriptions, we need to truncate so no line wrap occurs

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: No line wrap

Scope: any bead that defines dashboard output

Fixtures:
- pass: Line width is read from the terminal on every redraw and descriptions are truncated with an ellipsis.
- fail: Long descriptions wrap onto a second indented line.

## ANC-013

Anchor: Each dashboard row must contain only an icon, a task id, an age, a tag (a marker counts as part of the tag) and a description.

> avoid any text that is not necessary. concise and narrower

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 3.

Summary: No unnecessary text

Scope: any bead that defines dashboard output

Fixtures:
- pass: A row is: icon, id, age, tag, description.
- fail: Each row also shows the assignee name, bead status and full bead labels.

## ANC-014

Anchor: After the atm-bd-console app has started, no error may stop it: each error must be shown on the dashboard and the app must continue with its next tick; only a launch check may exit, with a message.

> tui can not fail after it starts.  error w/ good message when launch is fine.  after launch, errors should be displayed and it continues w/ best effort.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 3.

Summary: App never fails after launch

Scope: any bead that defines error handling in the tick, the app loop or the CLI

Fixtures:
- pass: A failed bd read during a tick appears in the dashboard's Errors section and the next tick runs on schedule.
- fail: If atm task list fails mid-run, the app exits with code 2.

## ANC-015

Anchor: Every 10-minute cycle atm-bd-console must check each pushed bead branch for merge conflicts against its PR target with git (no GitHub call), report a conflict to the assignee naming the conflicted files, and hold that bead's dispatch.

> checking for mergeability is important.  big mistake agents make.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Merge check each cycle

Scope: any bead that defines the reminder cycle, holds or the PRs tab

Fixtures:
- pass: Each cycle runs git fetch and git merge-tree --write-tree origin/<pr_target> origin/<branch>; exit 1 records the files in merge.json, holds the dispatch and tells the assignee.
- fail: Mergeability is read from GitHub's mergeable field for each PR.

## ANC-016

Anchor: atm-bd-console must report a red CI state only for the head of the top layer of the phase stack, to the lead, and must not report or act on CI state of lower layers.

> ci failed is tricky w/ stack.  we do not fix lower layers, we do need to report if top of stack is red.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Report red stack top only

Scope: any bead that reads or reports CI state

Fixtures:
- pass: The top PR's head rollup is FAILURE: the lead gets one problem for it; a red lower layer appears only in the PRs tab with no message.
- fail: Each red layer's dev is told to fix CI on that layer.

## ANC-017

Anchor: When GitHub is disabled or unavailable (no connectivity, timeout, rejected), atm-bd-console must still dispatch, check merges and send problems, and must show the GitHub state in the status bar.

> yes, if gh disabled or not available (no connectivity, timeout, rejected) we proceed with work.  gh status should show up in status bar at bottom.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Work proceeds without GitHub

Scope: any bead that makes a GitHub call or reads the github setting

Fixtures:
- pass: With github = off the tick dispatches sanity from bd and git facts and the status bar reads gh: off.
- fail: A GitHub timeout makes the tick skip dispatch until the next successful call.

## ANC-018

Anchor: atm-bd-console must have a setting that turns off GraphQL (falling back to REST) and one that turns off every GitHub call.

> we should add some config flags that disable graphql and optionally gh so we can shut them down if necessary.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: GitHub off switches

Scope: any bead that defines configuration or a GitHub call

Fixtures:
- pass: github = graphql | rest | off in .atm-bd/console.toml, overridden by --github <mode>.
- fail: GitHub access can only be stopped by revoking the gh token.

## ANC-019

Anchor: The status bar must show the GitHub state, done and total work as a count, percent and bar, where total includes the average findings per finished sprint projected onto unfinished sprints, and the phase's active time excluding time with nothing in progress.

> we should probably define a few metrics for status bar at bottom including , # & % complete (project average # of findings per sprint for all unfinished sprints) i.e. 2/8 [xx-----] , time (how long phase has been running - paused time).

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 3.

Summary: Status bar metrics

Scope: any bead that defines the status bar or its metrics

Fixtures:
- pass: The status bar shows the GitHub state (gh: graphql ✅), done/total beads as 3/9 with a bar and 33% where total adds each unfinished sprint's projected findings (average findings per finished sprint), and active 14h20m counting only time with a bead in progress.
- fail: The status bar shows sprints closed out of sprints planned and wall-clock time since the root was created.

## ANC-020

Anchor: atm-bd-console must show times on screen in 24-hour local time and store every time in logs and JSON files in UTC.

> time in 24h local time.  logs/under the hood in utc.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Local 24h on screen, UTC stored

Scope: any bead that shows or stores a time

Fixtures:
- pass: The status bar shows 14:04:31 and snapshot.json stores 2026-10-14T21:04:31Z.
- fail: snapshot.json stores 2:04 PM local time.

## ANC-021

Anchor: atm-bd-console must show each agent's status (busy, stalling, blocked, offline) as an icon next to its name and must not send messages about agent status, which ATM already nudges and escalates.

> stalled w/ active task, atm nudge should kick in, atm already has escellation in place.  so this should just show up as busy/stalling/blocked/offline w/ appropriate icons next to name

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: Agent status shown, not messaged

Scope: any bead that defines agent status or messages about agents

Fixtures:
- pass: An agent idle with a task shows a yellow half circle next to its name and no message is sent.
- fail: An agent idle 15 minutes with a task triggers a message to the lead.

## ANC-022

Anchor: Everything atm-bd-console shows on screen must come from JSON files it writes (a current snapshot plus detail files), so oversight can read the same data.

> Note all the data we are showing in tui should be in json. for oversight to read.  I expect there is a current state snapshot, then more extensive separate files for things like ci failure data, ...

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: locked. Version 2.

Summary: TUI data is JSON

Scope: any bead that adds something to the dashboard or tabs

Fixtures:
- pass: render.py reads only snapshot.json and the detail files merge.json, ci.json and prs.json; a CI failure's check names are in ci.json.
- fail: The PRs tab calls gh itself and shows results that are written nowhere.

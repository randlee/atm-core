# Anchors - phase-current

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `doc/plan/phase-current/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | proposed | No logic chains | 2026-10-14 |
| ANC-002 | proposed | Business logic in one place | 2026-10-14 |
| ANC-003 | proposed | Same behavior cron or TUI | 2026-10-14 |
| ANC-004 | proposed | Production rules only | 2026-10-14 |
| ANC-005 | proposed | No bd list --all | 2026-10-14 |
| ANC-006 | proposed | No gh calls when idle | 2026-10-14 |
| ANC-007 | proposed | --as mandatory | 2026-10-14 |
| ANC-008 | proposed | Per-agent queue shown | 2026-10-14 |
| ANC-009 | proposed | Non-sprint tasks marked | 2026-10-14 |
| ANC-010 | proposed | Escalation cadence | 2026-10-14 |
| ANC-011 | proposed | Phase end stops polling | 2026-10-14 |
| ANC-012 | proposed | No line wrap | 2026-10-14 |
| ANC-013 | proposed | No unnecessary text | 2026-10-14 |

## ANC-001

Anchor: Each atm-bd-console dispatch, hold, marker or section rule must be a single check on snapshot fields that does not depend on the result of any other rule.

> avoid complicated logic chains.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: No logic chains

Scope: any bead that defines or changes a dispatch, hold, escalation, marker or section rule

Fixtures:
- pass: Each dispatch rule is one check on snapshot fields: stage is sanity or QA, the bead has a branch, one PR is open, the worktree exists.
- fail: QA is dispatched only if sanity passed, and if that check fails the dispatcher then checks CI before deciding whether to retry the stack-top check.

## ANC-002

Anchor: Every atm-bd-console dispatch, escalation and section rule must be implemented in exactly one module, and the app, the cron mode and bead-queues must call that module rather than contain their own copy of any rule.

> business logic defined clearly in a single place.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Business logic in one place

Scope: any bead that adds code to render.py, cli.py, app.py or bead-queues

Fixtures:
- pass: All dispatch and section rules live in dispatch.py and model.py; render.py, cli.py and bead-queues only call them.
- fail: bead-queues keeps its own copy of the section rules so it can run without the app installed.

## ANC-003

Anchor: Running atm-bd-console as the cron job and as the TUI app must produce the same dispatches, messages and state for the same beads and tasks.

> identical behavior when run as cron or tui

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Same behavior cron or TUI

Scope: any bead that changes the tick, the app loop or the cron mode

Fixtures:
- pass: The app loop and --once --headless call the same tick function and a test runs both against one fixture and compares dispatches, messages and state.
- fail: The cron mode skips escalation batching to stay fast; only the app batches resends.

## ANC-004

Anchor: The atm-bd-console dispatcher must not hold or skip a sanity or QA dispatch for any reason that the production atm-bd-orchestration rules do not already use.

> we are working off a set of rules already in production.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Production rules only

Scope: any bead that defines when sanity or QA is dispatched or held

Fixtures:
- pass: The dispatcher sends sanity or QA when bd ready lists the bead, the branch has one open PR and the worktree exists; nothing else holds it.
- fail: QA waits until the PR's CI is green and the layer is the top of the stack.

## ANC-005

Anchor: atm-bd-console must never run `bd list --all`.

> Do NOT use `bd list --all`

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: No bd list --all

Scope: any bead that adds a bd read

Fixtures:
- pass: Each tick reads open beads with bd list -l phase-bh and fetches needed closed ids with one batched bd show.
- fail: Each tick runs bd list --all to find closed predecessors.

## ANC-006

Anchor: atm-bd-console must make no GitHub call on a tick that dispatches nothing.

> I thought there were no gh calls

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: No gh calls when idle

Scope: any bead that adds a gh or GitHub API call

Fixtures:
- pass: An idle tick reads only bd and atm; GitHub is called once per sanity or QA dispatch.
- fail: Each tick fetches open PRs from GitHub to refresh the dashboard.

## ANC-007

Anchor: atm-bd-console must refuse to start without `--as` and must print a message explaining what `--as` is.

> I would prefer that bead-dispatch makes --as a mandetory arg w/ good console message if it is not set

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: --as mandatory

Scope: any bead that defines the CLI arguments or the assigning identity

Fixtures:
- pass: Without --as the CLI exits 3 and prints what --as is and an example command.
- fail: Without --as the CLI uses $ATM_IDENTITY as the lead.

## ANC-008

Anchor: The dashboard must show, for each agent, its total number of assigned tasks and at most 3 of those tasks.

> what i want to see is what tasks agents are assigned. i.e. queue per agent (up to a limit around ~3. if > 3 tasks, I still want to know queue depth).

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Per-agent queue shown

Scope: any bead that defines the agent blocks

Fixtures:
- pass: Each agent block shows the agent's queue depth and up to 3 task rows, then '… +N more'.
- fail: The dashboard lists only the running task of each agent.

## ANC-009

Anchor: The dashboard must mark each assigned task whose bead is not part of the current phase's sprints.

> I also want to know if these are not sprint tasks.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Non-sprint tasks marked

Scope: any bead that defines the snapshot rows or the agent blocks

Fixtures:
- pass: A task whose bead's top epic is not the phase root shows a red marker; a task with no bead shows a yellow marker.
- fail: Tasks are listed by agent without any phase check.

## ANC-010

Anchor: Unresolved problems must be resent to the lead together in one message 10 minutes after the first escalation, and every 10 minutes after that.

> one message per problem, then resent after 10 minutes. if more issues show up, batch them into same 10 minute interval after 1st send.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Escalation cadence

Scope: any bead that defines escalation or hold messages

Fixtures:
- pass: Problems at 1m, 2m and 3m are sent at 1m, 2m and 3m, and resent together in one message at 11m.
- fail: Each unresolved problem is resent individually every tick until resolved.

## ANC-011

Anchor: Once the phase root bead is closed, atm-bd-console must not poll, dispatch or send messages, whether it runs as the app or as the cron job.

> app would stop polling and cron would do nothing when phase ended.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: Phase end stops polling

Scope: any bead that defines the tick start, the app loop or the cron mode

Fixtures:
- pass: When the root bead is closed, the app stops ticking and shows closed; the cron run exits 0 after the root check.
- fail: When the root closes, the app keeps polling every 60 s and shows no new rows.

## ANC-012

Anchor: Every dashboard line must fit the terminal width without wrapping, with descriptions truncated to fit.

> for all descriptions, we need to truncate so no line wrap occurs

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: No line wrap

Scope: any bead that defines dashboard output

Fixtures:
- pass: Line width is read from the terminal on every redraw and descriptions are truncated with an ellipsis.
- fail: Long descriptions wrap onto a second indented line.

## ANC-013

Anchor: Each dashboard row must contain only an icon, a task id, an age, a tag and a description.

> avoid any text that is not necessary. concise and narrower

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 2.

Summary: No unnecessary text

Scope: any bead that defines dashboard output

Fixtures:
- pass: A row is: icon, id, age, tag, description.
- fail: Each row also shows the assignee name, bead status and full bead labels.

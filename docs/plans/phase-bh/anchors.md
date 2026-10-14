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

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: No logic chains

Scope: any bead that defines or changes a dispatch, hold, escalation, marker or section rule

## ANC-002

Anchor: Every atm-bd-console dispatch, escalation and section rule must be implemented in exactly one module, and the app, the cron mode and bead-queues must call that module rather than contain their own copy of any rule.

> business logic defined clearly in a single place.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Business logic in one place

Scope: any bead that adds code to render.py, cli.py, app.py or bead-queues

## ANC-003

Anchor: Running atm-bd-console as the cron job and as the TUI app must produce the same dispatches, messages and state for the same beads and tasks.

> identical behavior when run as cron or tui

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Same behavior cron or TUI

Scope: any bead that changes the tick, the app loop or the cron mode

## ANC-004

Anchor: The atm-bd-console dispatcher must not hold or skip a sanity or QA dispatch for any reason that the production atm-bd-orchestration rules do not already use.

> we are working off a set of rules already in production.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Production rules only

Scope: any bead that defines when sanity or QA is dispatched or held

## ANC-005

Anchor: atm-bd-console must never run `bd list --all`.

> Do NOT use `bd list --all`

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: No bd list --all

Scope: any bead that adds a bd read

## ANC-006

Anchor: atm-bd-console must make no GitHub call on a tick that dispatches nothing.

> I thought there were no gh calls

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: No gh calls when idle

Scope: any bead that adds a gh or GitHub API call

## ANC-007

Anchor: atm-bd-console must refuse to start without `--as` and must print a message explaining what `--as` is.

> I would prefer that bead-dispatch makes --as a mandetory arg w/ good console message if it is not set

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: --as mandatory

Scope: any bead that defines the CLI arguments or the assigning identity

## ANC-008

Anchor: The dashboard must show, for each agent, its total number of assigned tasks and at most 3 of those tasks.

> what i want to see is what tasks agents are assigned. i.e. queue per agent (up to a limit around ~3. if > 3 tasks, I still want to know queue depth).

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Per-agent queue shown

Scope: any bead that defines the agent blocks

## ANC-009

Anchor: The dashboard must mark each assigned task whose bead is not part of the current phase's sprints.

> I also want to know if these are not sprint tasks.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Non-sprint tasks marked

Scope: any bead that defines the snapshot rows or the agent blocks

## ANC-010

Anchor: Unresolved problems must be resent to the lead together in one message 10 minutes after the first escalation, and every 10 minutes after that.

> one message per problem, then resent after 10 minutes. if more issues show up, batch them into same 10 minute interval after 1st send.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Escalation cadence

Scope: any bead that defines escalation or hold messages

## ANC-011

Anchor: Once the phase root bead is closed, atm-bd-console must not poll, dispatch or send messages, whether it runs as the app or as the cron job.

> app would stop polling and cron would do nothing when phase ended.

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: Phase end stops polling

Scope: any bead that defines the tick start, the app loop or the cron mode

## ANC-012

Anchor: Every dashboard line must fit the terminal width without wrapping, with descriptions truncated to fit.

> for all descriptions, we need to truncate so no line wrap occurs

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: No line wrap

Scope: any bead that defines dashboard output

## ANC-013

Anchor: Each dashboard row must contain only an icon, a task id, an age, a tag and a description.

> avoid any text that is not necessary. concise and narrower

Said by Rand, planning session 2026-10-14 (fenix), 2026-10-14. Status: proposed. Version 1.

Summary: No unnecessary text

Scope: any bead that defines dashboard output

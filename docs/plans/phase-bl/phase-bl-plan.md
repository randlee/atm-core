# Phase BL — atm-bd-orchestration 0.18.5

## Goal and authority

Release 0.18.5 for the fourteen scoped issues: #379, #164, #365, #497, #375,
#492, #372, #126, #533, #498, #491, #359, #360, and #347. Code is in
`randlee/atm-bd-orchestration`, branch `integrate/phase-bl`, based on v0.18.4
(`86853d5`). This repository owns the plan and beads.

The refresh contract follows Rand's 2026-10-21 ruling:

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

Tests use injected clocks, controlled fakes, and deterministic inputs. They do
not use sleeps, threads, races, event ordering, load, or real bd/dolt/atm/gh.

## Boundaries and ownership

| Boundary | Producer | Consumers | Ownership |
| --- | --- | --- | --- |
| `BOUNDARY-Refresh` | bl-1 | bl-6, bl-7, bl-9 | refresh state, source stamps, `Snapshot.content_at` |
| `BOUNDARY-JevResult` | bl-2 | bl-5, bl-12 | ordered Jev JSONL and `JevErrorKind` |
| `BOUNDARY-DispatchHeld` | bl-3 | none | dispatch and bead-source held rows |
| `BOUNDARY-ProblemSources` | bl-10 | none | model, GitHub and atm-bd problem sources |
| `BOUNDARY-QaReviewers` | bl-4 | bl-7, bl-12 | reviewer metadata |
| `BOUNDARY-SanityRecord` | bl-5 | bl-7, bl-12 | sanity-history record and merge rule |
| `BOUNDARY-SanityMechanics` | bl-11 | none | task close and stack mechanics |
| `BOUNDARY-DashboardRender` | bl-6 | bl-9 | render, status, show |
| `BOUNDARY-ConsoleApp` | bl-7 | bl-9, bl-12 | console tabs and icons |
| `BOUNDARY-BdDeadline` | bl-8 | none until D1 | deadline-only command seam |

No boundary implies ownership of a consumer's implementation files. In
particular, bl-1 owns `cli.py` refresh entry; bl-6 owns `show.py` and
`test_show.py` and consumes only `Snapshot.content_at` and restored
`PollHealth` from bl-1.

## Three-wave execution and one stack

| Wave | Sprints | Hard API/artifact dependency |
| --- | --- | --- |
| 1 | bl-1, bl-2, bl-3, bl-4, bl-8, bl-10, bl-11 | none; bl-8 dispatch begins only after D1 |
| 2 | bl-5, bl-6, bl-7 | bl-5 ← bl-2 JSONL; bl-6 ← bl-1 content/health; bl-7 ← bl-1 refresh and bl-4 reviewers |
| 3 | bl-9, bl-12 | bl-9 ← bl-6/bl-7; bl-12 ← bl-4/bl-5/bl-7 |

Critical path: three waves. Maximum implementation width: seven in wave 1.

The DAG is in `phase-bl.jsonl`; this table is its authoritative human view.
Development can proceed in parallel according to that DAG. At handoff there
is one append-only stack: `bl-1 → bl-2 → bl-3 → bl-4 → bl-8 → bl-10 → bl-11
→ bl-5 → bl-6 → bl-7 → bl-9 → bl-12`. Each PR targets the immediately
preceding layer. The stack writer rebases a ready branch on that layer and
merge-forwards its declared producers before linking it; no sprint is an
independent, unlinked delivery track. bl-9 is the higher parent that
merge-forwards the completed dashboard and console consumers; bl-12 follows
bl-9 for the final integration surface.

| Sprint | Boundary | PR target | Scope summary |
| --- | --- | --- | --- |
| bl-1 | Refresh | `integrate/phase-bl` | shared refresh and persisted data |
| bl-2 | JevResult | bl-1 | typed Jev JSONL producer |
| bl-3 | DispatchHeld | bl-2 | dispatch and bead source only |
| bl-4 | QaReviewers | bl-3 | QA/finding reviewer schema |
| bl-8 | BdDeadline | bl-4 | D1 deadline in `command.py` only |
| bl-10 | ProblemSources | bl-8 | model, GitHub, atm-bd sources |
| bl-11 | SanityMechanics | bl-10 | close and stack mechanics |
| bl-5 | SanityRecord | bl-11 | Jev consumer and history |
| bl-6 | DashboardRender | bl-5 | render, status, show |
| bl-7 | ConsoleApp | bl-6 | app tabs and icons |
| bl-9 | Refresh integration | bl-7 | refresh end-to-end tests and proof |
| bl-12 | Uniform integrations | bl-9 | Jev/history/console/reviewer integration |

## Decisions

D1–D4 remain open and block only the named work below. When any is ruled,
the lead creates a new decision-specific sprint; no current sprint assumes its
answer.

| Decision | Question | Current effect |
| --- | --- | --- |
| D1 / #533 | What BD read deadline is acceptable for the console command? | bl-8's sole `command.py` deadline change waits. |
| D2 / #492 | Should display-only “head not pushed” remain informational, and should PR-less pairs be merge-checked? | no current change; a new sprint follows a ruling. |
| D3 / #498 | Is notice on `rebase_required` sufficient, or must the bead reopen? | only that unruled behavior waits. |
| D4 / #126 | Which remaining candidate, gate, reviewer, link, supervision, escalation, and lookup proposals are wanted? | no current implementation work waits on it. |

D5 is ruled and is implemented mechanically by bl-5:

> "if jev-api failed (i.e. jev did not respond), the parallel llm evaluator's response is used. If both llm+jev respond a failure on either is failure on iteration #1. for iteration > 1, sanity-agent to triage (which should be done w/ background agent if sanity-agent has queue backlog)."

## Integration exits

bl-9 proves cron and console reuse exactly the same persisted report, stdout,
and exit outcome for `fresh` and `in_progress` reuse; it reports only
persisted state. bl-12 proves Jev JSONL flows through sanity history to the
console do-not-enter state and that reviewer metadata reaches the Quality
pane. Product behavior defects discovered by integration return to the owning
boundary sprint.

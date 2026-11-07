# Anchors - phase-bs

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bs/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Split too-large plans | 2026-11-07 |
| ANC-002 | locked | Short wave graph | 2026-11-07 |
| ANC-003 | locked | Child beads per subagent | 2026-11-07 |
| ANC-004 | locked | Recommend child beads | 2026-11-07 |
| ANC-005 | locked | Model class per child | 2026-11-07 |
| ANC-006 | locked | Dev runs children | 2026-11-07 |
| ANC-007 | locked | One branch, no subagent PRs | 2026-11-07 |
| ANC-008 | locked | Subagent focused lint/test | 2026-11-07 |
| ANC-009 | locked | Full lint/test on fast agent | 2026-11-07 |
| ANC-010 | locked | Final rebase after green | 2026-11-07 |
| ANC-011 | locked | No full rerun after rebase | 2026-11-07 |
| ANC-012 | locked | Parent closes after children | 2026-11-07 |
| ANC-013 | locked | One sanity bead per sprint | 2026-11-07 |
| ANC-014 | locked | Commit per dev bead | 2026-11-07 |
| ANC-015 | locked | One QA bead per sprint | 2026-11-07 |
| ANC-016 | locked | No plan-anchoring mention | 2026-11-07 |
| ANC-017 | locked | Necessary words only | 2026-11-07 |

## ANC-001

Anchor: A phase plan too large for one sprint must be split into multiple sprints.

> If plan is too large it MUST be split.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Split too-large plans

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-002

Anchor: The plan split must place unrelated, non-intersecting work in separate sprints so the sprint dependency graph stays short.

> The split should be done to optimize horizontal waves (keep height of graph short), unrelated/un-intersecting work split into separate sprints.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Short wave graph

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-003

Anchor: A large sprint of closely related work must be split into child beads under one dev, one child bead per background subagent.

> Large sprints w/ closely related work should be split into child beads one per [background] subagent.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Child beads per subagent

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-004

Anchor: atm-bd-orchestration planning guidance must recommend child beads for concentrated pockets of closely related work managed by a single dev agent.

> we are adding a new option which we want to highly recommend for efficiency.  It is important to create a well balanced plan of waves with concentrated pockets of work managed by a single dev agent

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Recommend child beads

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-005

Anchor: A child bead must declare its subagent's model with the existing beads class fast, normal or frontier.

> Use already defined 'fast','normal','frontier' classification in beads.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Model class per child

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-006

Anchor: The sprint's dev must run each child bead with a background subagent.

> I think it should be a strategy as part of planning to encourage agents to delegate work to multiple subagents (and specfically handle merging subagent work into their worktree)  when therre is a larger task.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Dev runs children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-007

Anchor: Subagent work must not get its own PR; the sprint's dev must deliver one working, tested branch.

> We do not need pr for subagent work.  Primary dev agent is responsible for delivering a single working tested branch.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: One branch, no subagent PRs

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-008

Anchor: Each background subagent must run focused lint and tests on its own worktree.

> Additionally background sub-agents should do small focused test/lint on their worktree.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Subagent focused lint/test

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-009

Anchor: The sprint's dev must delegate the full lint and test run to a background fast-class agent.

> Dev should delegate running full lint/test to background 'fast' agent.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Full lint/test on fast agent

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-010

Anchor: The dev's final rebase onto the top of the stack must happen after full lint and test pass.

> Final rebase AFTER tests/lint pass.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Final rebase after green

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-011

Anchor: The dev must not rerun full lint and test after the final rebase unless the rebase required resolving conflicts in code the sprint wrote.

> Rebasing on top of stack AND make sure tests/lint pass 100% are impossible to both achieve if 5-15 minutes of testing are run after rebase. I have seen agents looping over run tests <-> rebase 10+ times in a row before they can land the stack. 4e could add 'unless conflicts with code written must be resolved' or similar.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: No full rerun after rebase

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-012

Anchor: The dev task must state in a few words that the parent bead closes after its children, with enforcement left to the bd close failure and no hook or script.

> beads does NOT allow closing parent w/ open children.  This does NOT need much explaination, the agent will be told he can not close parent first so few words in task/prompt need to talk about it.  No hook/python for this at all outside of returning close fail message from failed 'bd close'.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Parent closes after children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-013

Anchor: Each sprint must have exactly one sanity bead, and it must name every dev bead of the sprint.

> There is ONE sanity bead for all dev work.  the sanity task/bead would specify all dev beads for split.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: One sanity bead per sprint

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-014

Anchor: Every dev bead, parent and child, must record its git commit so sanity can match each commit to its bead.

> we should require git commit# per dev bead so sanity can coorelate commit to dev task/bead

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Commit per dev bead

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-015

Anchor: Each sprint must have exactly one QA bead and one QA run on the sprint's single worktree.

> Single qa bead, subagents would handle all work on single worktree as single qa run.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: One QA bead per sprint

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

## ANC-016

Anchor: Docs, templates and prompts under atm-bd-orchestration must not mention plan-anchoring.

> don't mention plan-anchoring in atm-bd

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: No plan-anchoring mention

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Deterministic check to pair with the development gate: grep -ri plan-anchoring plugins/atm-bd-orchestration returns nothing

## ANC-017

Anchor: Prompts and task text for the subtask strategy must contain only necessary words.

> in general prompts/tasks must be written with NO unnecessary words.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 1.

Summary: Necessary words only

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

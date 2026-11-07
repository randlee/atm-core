# Anchors - phase-bt

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bt/intent.md`

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

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Split too-large plans

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-te91`, plan questions `atm-te91.1`, dev questions `atm-te91.2`

Fixtures:
- pass: The planning guide states that a plan too large must be split into multiple sprints.
- fail: The planning guide lets an oversized plan stay one sprint.

## ANC-002

Anchor: The plan split must place unrelated, non-intersecting work in separate sprints so the sprint dependency graph stays short.

> The split should be done to optimize horizontal waves (keep height of graph short), unrelated/un-intersecting work split into separate sprints.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Short wave graph

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-d42v`, plan questions `atm-d42v.1`, dev questions `atm-d42v.2`

Fixtures:
- pass: planning.md tells planners to put work items that share no files or code into separate sprints.
- fail: planning.md bundles unrelated work items that touch no common files into one sprint.

## ANC-003

Anchor: A large sprint of closely related work must be split into child beads under one dev, one child bead per background subagent.

> Large sprints w/ closely related work should be split into child beads one per [background] subagent.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Child beads per subagent

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-u194`, plan questions `atm-u194.1`, dev questions `atm-u194.2`

Fixtures:
- pass: planning.md directs that a large sprint of closely related work is split into child beads, one per background subagent.
- fail: planning.md splits a large closely related sprint into independent sibling sprints.

## ANC-004

Anchor: atm-bd-orchestration planning guidance must recommend child beads for concentrated pockets of closely related work managed by a single dev agent.

> we are adding a new option which we want to highly recommend for efficiency.  It is important to create a well balanced plan of waves with concentrated pockets of work managed by a single dev agent

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 3.

Summary: Recommend child beads

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-w53l`, plan questions `atm-w53l.1`, dev questions `atm-w53l.2`

Fixtures:
- pass: planning.md recommends child beads for concentrated pockets of closely related work, each pocket managed by a single dev agent.
- fail: planning.md presents child beads as an obscure option without recommending them.

## ANC-005

Anchor: A child bead must declare its subagent's model with the existing beads class fast, normal or frontier.

> Use already defined 'fast','normal','frontier' classification in beads.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Model class per child

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-w0y3`, plan questions `atm-w0y3.1`, dev questions `atm-w0y3.2`

Fixtures:
- pass: Each child bead declares its subagent model as fast, normal or frontier.
- fail: Child beads declare a new model field with values opus/sonnet/haiku.

## ANC-006

Anchor: The sprint's dev must run each child bead with a background subagent.

> I think it should be a strategy as part of planning to encourage agents to delegate work to multiple subagents (and specfically handle merging subagent work into their worktree)  when therre is a larger task.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Dev runs children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-umb4`, plan questions `atm-umb4.1`, dev questions `atm-umb4.2`

Fixtures:
- pass: The dev template tells the dev to run each child bead with its own background subagent.
- fail: The dev template has the lead dispatch each child bead to a separate dev agent.

## ANC-007

Anchor: Subagent work must not get its own PR; the sprint's dev must deliver one working, tested branch.

> We do not need pr for subagent work.  Primary dev agent is responsible for delivering a single working tested branch.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: One branch, no subagent PRs

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-66vl`, plan questions `atm-66vl.1`, dev questions `atm-66vl.2`

Fixtures:
- pass: Subagent work is merged into the dev's branch; only the dev's branch gets a PR.
- fail: Each subagent opens its own GitHub pull request for its child bead.

## ANC-008

Anchor: Each background subagent must run focused lint and tests on its own worktree.

> Additionally background sub-agents should do small focused test/lint on their worktree.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Subagent focused lint/test

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-kha6`, plan questions `atm-kha6.1`, dev questions `atm-kha6.2`

Fixtures:
- pass: Each subagent runs a small, focused test and lint pass on its own worktree.
- fail: Subagents skip testing, or each runs the full suite on its own worktree.

## ANC-009

Anchor: The sprint's dev must delegate the full lint and test run to a background fast-class agent.

> Dev should delegate running full lint/test to background 'fast' agent.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Full lint/test on fast agent

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-plmh`, plan questions `atm-plmh.1`, dev questions `atm-plmh.2`

Fixtures:
- pass: The dev template tells the dev to delegate the full lint/test run to a background fast agent.
- fail: The dev runs the full lint/test suite in its own session.

## ANC-010

Anchor: The dev's final rebase onto the top of the stack must happen after full lint and test pass.

> Final rebase AFTER tests/lint pass.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Final rebase after green

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-ff88`, plan questions `atm-ff88.1`, dev questions `atm-ff88.2`

Fixtures:
- pass: The dev rebases onto the top of the stack only after full lint and test pass.
- fail: The dev rebases onto the top of the stack, then runs full lint and test.

## ANC-011

Anchor: The dev must not rerun full lint and test after the final rebase unless the rebase required resolving conflicts in code the sprint wrote.

> Rebasing on top of stack AND make sure tests/lint pass 100% are impossible to both achieve if 5-15 minutes of testing are run after rebase. I have seen agents looping over run tests <-> rebase 10+ times in a row before they can land the stack. 4e could add 'unless conflicts with code written must be resolved' or similar.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 3.

Summary: No full rerun after rebase

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-m1dj`, plan questions `atm-m1dj.1`, dev questions `atm-m1dj.2`

Fixtures:
- pass: After the final rebase the dev skips the full lint and test rerun, unless the rebase required resolving conflicts in code the sprint wrote.
- fail: After the final rebase the dev reruns the full lint and test suite.

## ANC-012

Anchor: The dev task must state in a few words that the parent bead closes after its children, with enforcement left to the bd close failure and no hook or script.

> beads does NOT allow closing parent w/ open children.  This does NOT need much explaination, the agent will be told he can not close parent first so few words in task/prompt need to talk about it.  No hook/python for this at all outside of returning close fail message from failed 'bd close'.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Parent closes after children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-q2b3`, plan questions `atm-q2b3.1`, dev questions `atm-q2b3.2`

Fixtures:
- pass: The dev task text says in one short sentence that the parent bead cannot close before its children.
- fail: The dev task text spends a paragraph explaining child/parent close order.

## ANC-013

Anchor: Each sprint must have exactly one sanity bead, and it must name every dev bead of the sprint.

> There is ONE sanity bead for all dev work.  the sanity task/bead would specify all dev beads for split.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: One sanity bead per sprint

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-tyf5`, plan questions `atm-tyf5.1`, dev questions `atm-tyf5.2`

Fixtures:
- pass: A split sprint has one sanity bead covering the parent dev bead and every child bead.
- fail: The pour creates one sanity bead per child bead.

## ANC-014

Anchor: Every dev bead, parent and child, must record its git commit so sanity can match each commit to its bead.

> we should require git commit# per dev bead so sanity can coorelate commit to dev task/bead

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Commit per dev bead

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Beads: anchor `atm-ggaq`, plan questions `atm-ggaq.1`, dev questions `atm-ggaq.2`

Fixtures:
- pass: Each dev bead, parent and child, records the git commit of its work.
- fail: Only the parent dev bead records a commit; child beads have none.

## ANC-015

Anchor: Each sprint must have exactly one QA bead and one QA run on the sprint's single worktree.

> Single qa bead, subagents would handle all work on single worktree as single qa run.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 3.

Summary: One QA bead per sprint

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-p2kg`, plan questions `atm-p2kg.1`, dev questions `atm-p2kg.2`

Fixtures:
- pass: Each sprint has exactly one QA bead, and QA is one run on the sprint's single worktree.
- fail: The pour creates a QA bead per child bead.

## ANC-016

Anchor: Docs, templates and prompts under atm-bd-orchestration must not mention plan-anchoring.

> don't mention plan-anchoring in atm-bd

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: No plan-anchoring mention

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Deterministic check to pair with the development gate: grep -ri plan-anchoring plugins/atm-bd-orchestration returns nothing

Beads: anchor `atm-nmob`, plan questions `atm-nmob.1`, dev questions `atm-nmob.2`

Fixtures:
- pass: atm-bd-orchestration templates and docs make no reference to plan-anchoring.
- fail: The dev template tells the dev to keep bead text under the plan-anchoring cap.

## ANC-017

Anchor: Prompts and task text for the subtask strategy must contain only necessary words.

> in general prompts/tasks must be written with NO unnecessary words.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 3.

Summary: Necessary words only

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2

Beads: anchor `atm-eynt`, plan questions `atm-eynt.1`, dev questions `atm-eynt.2`

Fixtures:
- pass: Every sentence this sprint adds to a prompt or task template for child beads carries only necessary words.
- fail: The dev template adds a paragraph explaining why child beads exist.

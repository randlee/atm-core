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
| ANC-018 | locked | Sprint fits one Jev call | 2026-11-15 |
| ANC-019 | locked | Subtask or reference split | 2026-11-15 |
| ANC-020 | locked | Jev skips reference beads | 2026-11-15 |
| ANC-021 | locked | No diff slicing | 2026-11-15 |
| ANC-022 | locked | Subtask stage typed | 2026-11-15 |
| ANC-023 | locked | Short focused sprints | 2026-11-15 |
| ANC-024 | locked | Only named issues | 2026-11-15 |
| ANC-025 | locked | Reference stage typed | 2026-11-15 |

## ANC-001

Anchor: A phase plan too large for one sprint must be split into multiple sprints.

> If plan is too large it MUST be split.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Split too-large plans

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 adds bead models and the size check; the child-bead planning guidance is bt-2's (atm-bt-2.sub-1)

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 adds bead models and the size check; the child-bead planning guidance is bt-2's (atm-bt-2.sub-1)

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 adds bead models and the size check; the child-bead planning guidance is bt-2's (atm-bt-2.sub-1)

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 adds bead models and the size check; the child-bead planning guidance is bt-2's (atm-bt-2.sub-1)

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow

Beads: anchor `atm-w0y3`, plan questions `atm-w0y3.1`, dev questions `atm-w0y3.2`

Fixtures:
- pass: Each child bead declares its subagent model as fast, normal or frontier.
- fail: Child beads declare a new model field with values opus/sonnet/haiku.

## ANC-006

Anchor: The sprint's dev must run each child bead with a background subagent.

> I think it should be a strategy as part of planning to encourage agents to delegate work to multiple subagents (and specifically handle merging subagent work into their worktree)  when therre is a larger task.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Dev runs children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

Beads: anchor `atm-m1dj`, plan questions `atm-m1dj.1`, dev questions `atm-m1dj.2`

Fixtures:
- pass: After the final rebase the dev skips the full lint and test rerun, unless the rebase required resolving conflicts in code the sprint wrote.
- fail: After the final rebase the dev reruns the full lint and test suite.

## ANC-012

Anchor: The dev task must state in a few words that the parent bead closes after its children, with enforcement left to the bd close failure and no hook or script.

> beads does NOT allow closing parent w/ open children.  This does NOT need much explanation, the agent will be told he can not close parent first so few words in task/prompt need to talk about it.  No hook/python for this at all outside of returning close fail message from failed 'bd close'.

Said by Rand, planning session 2026-11-07, issue atm-bd#937 subtask beads, 2026-11-07. Status: locked. Version 2.

Summary: Parent closes after children

Scope: atm-bd-orchestration planning guidance, bead schema, dev/sanity/QA templates and hooks for subtask beads (#937)

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938); child-bead planning, dev and QA flow is atm-bt-2
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch how a dev runs, merges, tests, rebases or closes child beads; that is atm-bt-2.sub-2

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 ships the ceremony screen (#938) and plans no sanity beads; sprint sanity beads are atm-bt-2 scope
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch the sanity bead or commit recording; that is atm-bt-2.sub-3

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch the sanity bead or commit recording; that is atm-bt-2.sub-3

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 does not touch the QA bead or QA run; that is atm-bt-2.sub-3

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

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow

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
- `atm-bt-3`: atm-bt-3 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-4`: atm-bt-4 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-5`: atm-bt-5 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-6`: atm-bt-6 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-7`: atm-bt-7 fixes named bugs and does not touch child-bead planning, dev, sanity or QA flow
- `atm-bt-8`: bt-8 writes no subtask prompts or task text; its planning.md edit is two sentences

Beads: anchor `atm-eynt`, plan questions `atm-eynt.1`, dev questions `atm-eynt.2`

Fixtures:
- pass: Every sentence this sprint adds to a prompt or task template for child beads carries only necessary words.
- fail: The dev template adds a paragraph explaining why child beads exist.

## ANC-018

Anchor: Every sprint and subtask bead's deliverable text must fit within jev_client's 96,000-byte Jev request limit (MAX_REQUEST_BYTES).

> I don't want sprints larger than Jev context.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 4.

Summary: Sprint fits one Jev call

Scope: edits validate-plan, bead_schema.py, or the plan size rule in planning.md

Deterministic check to pair with the development gate: validate-plan test: an oversized sprint bead fails with the typed oversize code

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-u5h6`, plan questions `atm-u5h6.1`, dev questions `atm-u5h6.2`

Fixtures:
- pass: Every phase-bt sprint and subtask bead's deliverable text fits within MAX_REQUEST_BYTES: `validate-plan --phase bt` passes, and test_validate_plan_oversized_sprint shows an oversized sprint is refused.
- fail: A sprint's deliverable text may exceed the Jev request limit; sanity splits it.

## ANC-019

Anchor: A sprint larger than the Jev context must be reduced by splitting it into subtask beads or by moving its code samples or reference documentation into reference beads.

> I don't want sprints larger than Jev context.  Sub-tasks OR reference beads with code samples (or reference documentation) which would be skipped by Jev.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 3.

Summary: Subtask or reference split

Scope: edits the oversize refusal text in validate-plan or jev_client, or the size rule in planning.md

Deterministic check to pair with the development gate: grep the refusal fix text and planning.md for both remedies

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-opat`, plan questions `atm-opat.1`, dev questions `atm-opat.2`

Fixtures:
- pass: The oversize refusal tells the planner to split the sprint into subtask beads or move its code samples into reference beads; test_oversize_refusal_names_both_remedies asserts both remedies.
- fail: The oversize refusal tells the planner to trim the deliverable text until it fits.

## ANC-020

Anchor: The content of a reference bead (a bead holding code samples or reference documentation for a sprint) must never be included in a request sent to the Jev sanity judge.

> I don't want sprints larger than Jev context.  Sub-tasks OR reference beads with code samples (or reference documentation) which would be skipped by Jev.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 3.

Summary: Jev skips reference beads

Scope: edits jev_client.py request building or the reference bead model

Deterministic check to pair with the development gate: test: a deliverable with a linked reference bead produces a Jev request that omits the reference text

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-zazu`, plan questions `atm-zazu.1`, dev questions `atm-zazu.2`

Fixtures:
- pass: jev_client builds the Jev request from the deliverable text and diff only; test_reference_bead_not_in_jev_request asserts the request omits every linked reference bead's text.
- fail: jev_client appends each linked reference bead's text to the Jev request evidence.

## ANC-021

Anchor: Work too large for one 96,000-byte Jev request must be split into subtask beads when the plan is written; jev_client must never cut it into diff slices.

> This won't work. We need to split into sub-tasks

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 4.

Summary: No diff slicing

Scope: edits jev_client.py oversize handling or part evaluation

Deterministic check to pair with the development gate: grep jev_client.py: no split_request or part evaluation; test: oversized request returns the typed refusal

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-v6qf`, plan questions `atm-v6qf.1`, dev questions `atm-v6qf.2`

Fixtures:
- pass: Work too large for one Jev request is split into subtask beads in the plan; split_request is deleted from jev_client, and test_oversized_request_refused shows an oversized request returns the typed refusal.
- fail: An oversized request is cut by file and hunk into parts that are judged separately.

## ANC-022

Anchor: The bead schema (bead_schema.py) must define a dedicated checked model for the stage:subtask bead stage.

> subtask and reference should both show up.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 3.

Summary: Subtask stage typed

Scope: edits bead_schema.py stage models

Deterministic check to pair with the development gate: MODELS_BY_STAGE maps both labels to dedicated models; schema test rejects a malformed subtask and reference bead

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-eb8i`, plan questions `atm-eb8i.1`, dev questions `atm-eb8i.2`

Fixtures:
- pass: MODELS_BY_STAGE maps stage:subtask to SubtaskBead, which checks its parent and difficulty; test_subtask_bead_model rejects a subtask bead without a parent.
- fail: stage:subtask stays mapped to the plain Bead model.

## ANC-023

Anchor: Each sprint's diff must add only the code its named issues require, with no new scripts, checks or process steps.

> Keep them short and focused.  This repo already has 10x more code than it should.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 4.

Summary: Short focused sprints

Scope: every bt-3..bt-8 sprint diff

Deterministic check to pair with the development gate: diff review: every added file and function traces to a named issue's deliverable

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 was planned 2026-11-07 under ANC-001..017, before the 2026-11-15 release-list ruling
- `atm-bt-2`: atm-bt-2 was planned 2026-11-07 under ANC-001..017, before the 2026-11-15 release-list ruling

Beads: anchor `atm-uav5`, plan questions `atm-uav5.1`, dev questions `atm-uav5.2`

Fixtures:
- pass: `git diff --stat main...HEAD` lists only sanity-split and tests/test_sanity_split.py: the two functions issue #986 names and their tests, with no new script, check or process step.
- fail: The sprint also adds a helper script and a new lint check for related cases.

## ANC-024

Anchor: Each of issues #938, #937, #979, #986, #928, #922, #907, #906, #904, #932 and #935 must be named by at least one sprint in the phase-bt plan.

> Create sprints for all these items as outlined above.  Keep them short and focused.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 9.

Summary: Only named issues

Scope: the phase-bt plan: the titles of its atm-bt-* sprint beads

Deterministic check to pair with the development gate: every issue in the release list (#979 #986 #928 #922 #907 #906 #904 #932 #935 #937 #938) appears in at least one atm-bt-* sprint bead title

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 was planned 2026-11-07 under ANC-001..017, before the 2026-11-15 release-list ruling
- `atm-bt-2`: atm-bt-2 was planned 2026-11-07 under ANC-001..017, before the 2026-11-15 release-list ruling

Beads: anchor `atm-sozk`, plan questions `atm-sozk.1`, dev questions `atm-sozk.2`

Fixtures:
- pass: `bd list --label phase-bt --type task` shows a sprint naming each issue: bt-1 #938; bt-2 and bt-8 #937; bt-3 #979 #986; bt-4 #928; bt-5 #922 #907 #906; bt-6 #904 #932; bt-7 #935.
- fail: No phase-bt sprint names #935.

## ANC-025

Anchor: The bead schema (bead_schema.py) must define a dedicated checked model for the stage:reference bead stage.

> subtask and reference should both show up.

Said by Rand, planning session 2026-11-15 (phase-bt release scope), 2026-11-15. Status: locked. Version 4.

Summary: Reference stage typed

Scope: edits bead_schema.py stage models

Deterministic check to pair with the development gate: MODELS_BY_STAGE maps stage:reference to a dedicated model; schema test rejects a malformed reference bead

Excluded from (not judged; the cell passes as `excluded`):
- `atm-bt-1`: atm-bt-1 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-2`: atm-bt-2 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-3`: atm-bt-3 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-4`: atm-bt-4 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-5`: atm-bt-5 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-6`: atm-bt-6 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8
- `atm-bt-7`: atm-bt-7 does not touch sprint sizing, bead stages or jev_client; that work is atm-bt-8

Beads: anchor `atm-8sxr`, plan questions `atm-8sxr.1`, dev questions `atm-8sxr.2`

Fixtures:
- pass: MODELS_BY_STAGE maps stage:reference to ReferenceBead, which checks the bead it is linked from; test_reference_bead_model rejects an unlinked reference bead.
- fail: bead_schema.py has no stage:reference model; reference material is stored as a plain stage:plan bead.

# Orchestrating with Beads

For the lead: check the plan in beads, keep the dependency graph current, and
dispatch from `bd ready`. The plan itself (epic and sprint beads) is written
as in [`planning.md`](planning.md).

Never block on bureaucracy. Dev waits only on its prerequisites' sanity checks; QA, triage and
fixes run beside it. 100% of findings are closed before the phase closes, and
one or two fast agents keep up with the important and minor ones.

## Before Dispatch

1. `.claude/skills/atm-beads/scripts/validate-plan --phase <x>` exits 0
   (it runs `bd doctor` and the checks [`planning.md`](planning.md) "Checks"
   assigns to it), and the plan has passed plan review
   (`atm-bd-orchestration` "Plan Gate").
2. `bd ready -l phase-<x> -n 0` lists exactly the dev beads with no prerequisites.
3. `bd ready --explain` shows every other dev bead blocked by the
   sanity check beads of its prerequisites.
4. The dispatched agent reads its assignment from the bead: `bd show <bead>`
   gives the description, design, acceptance criteria and metadata (branch,
   `pr_target`, worktree).

## Dependencies

Plan time creates the sprint containers; `bead-groups` pours each one's
`dev ← sanity ← qa` group under it. At runtime quality-mgr pours one
`fix ← sanity ← qa` group under the sprint per blocking finding and files the
important and minor findings as finding beads.

| Edge | Type | Meaning |
| --- | --- | --- |
| sanity check → dev (or fix) | `blocks` | sanity check runs on the closed dev or fix bead |
| QA → sanity check | `blocks` | QA runs after the sanity check passes |
| QA → dev (or fix) | `validates` | records the bead the QA reviews |
| dependent dev → predecessor's sanity check | `blocks` | a dev bead waits on the sanity check of each required prerequisite |
| `parallel_safe` | none | no edge |
| fix → QA | `discovered-from` | records the QA that found its finding; blocks nothing |
| finding → QA | `discovered-from` | records the QA that found it; blocks nothing |
| fix → fix, finding → finding | `blocks` | only when one fix needs another first |

The sprint container is the parent of its groups; the phase feature is the
parent of the containers and of the important and minor findings, and an
ancestor of every other bead, not a blocker. Neither closes while a child is
open.

## Dev Sanity Check

Every dev bead is followed by a sanity check bead, which is blocked by the dev
bead and blocks every dev bead that requires it. Its assignment is
[`dev-sanity.md`](dev-sanity.md).

1. The dev agent completes its dev task and closes it; the task assigner receives the
   dev-task completion.
2. Lead assigns the sanity check.
3. If sanity check fails, the sanity member files every reported undone
   deliverable (never lint) as a child finding bead of the checked bead. It adds `blocks` edges only between those new beads, where one fix depends on another. It then reopens the checked
   bead and leaves the sanity check open, re-blocked by its edge. Lead reviews
   those child findings and gives the dev agent a dev-fix assignment.

4. Steps 1 to 3 repeat until the task is done.

The dev agent may instead declare failure, with the reason the work cannot be
completed. It sets the bead `blocked` with a `DEV_CANNOT_COMPLETE` note and does not close
it: a closed dev bead would release its sanity check.

## QA And Findings

Nothing waits on QA. A dev bead is released by its prerequisites'
sanity checks alone, so dev keeps moving while a layer is reviewed.

1. Sanity check N is green: the poured QA bead (a child of the sprint,
   `validates` dev N) is ready, and lead assigns it to quality-mgr with the
   pinned head SHA.
2. quality-mgr runs its review agents as background agents, as it does today,
   triages their results, and screens them with the `ceremony-finding-screen`
   procedure (`.claude/agents/ceremony-finding-screen.md`), also run as a
   background agent.
3. quality-mgr files every finding, screened or not. A blocking finding the
   screen keeps is poured with `bead-groups --findings` as a fix group under
   the sprint, its fix bead `discovered-from` the QA bead. Every other finding
   is a finding bead: parent the phase feature, `discovered-from` the QA bead,
   metadata `severity`, `stack` and `found_on_layer`. The screen verdict
   decides what happens to it:
   - `ceremony`: filed and closed at once, not fixed:
     `bd close <finding> --reason "ceremony: <reason>"`;
   - `concern_valid_remedy_ceremony`: filed open, with the remedy rewritten
     to the existing mechanism the screen names;
   - `keep` or `not_applicable`: filed open as reported.
   Every finding closes with a close reason, fixed or not.
4. If lead disagrees with a ceremony closure, lead reopens the finding:
   `bd reopen <finding> --reason "<why>"`.
5. Findings are `parallel_safe` by default. Add a `blocks` edge between two
   findings only when one fix needs the other first.
6. Each fix lands on the stack the finding was found on, one fix per layer.
   Fix layers can land between dev layers of the same stack.
7. A fix is assigned to a dev like a dev task: the poured fix bead (or an
   important or minor finding bead) is the task; a fix bead is followed by its
   group's sanity check and QA, as for a dev bead. Each QA round after the
   first reviews one small fix layer, so it is quick. A failed fix QA pours the
   next round's group; nothing is reopened.

Priority is the queue order. `bd ready` sorts by it, so a blocking finding is
next up for its assignee, ahead of the next dev bead.

| Severity | Priority | Typical assignee |
| --- | --- | --- |
| blocking | P1 | frontier / high-value dev |
| (planned dev beads) | P2 | frontier / high-value dev |
| important | P2 | fast agent |
| minor | P4 | fast agent |

Lead picks the team member for each finding. The assignee column is the usual
case, not a rule: an important but difficult finding may go to a frontier dev,
and a blocking doc fix may go to a fast agent.

## Passing A Bead To A Background Agent

This works the same for Claude and Codex agents. A background agent (a
subagent or child agent, whichever the harness provides) is never assumed to
run `bd`. The agent that starts it fetches the bead and pipes the content into
the background agent's prompt file:

```bash
{ printf '%s\n\n' "<instruction>"
  printf 'The bead:\n\n```json\n'
  bd show <bead> --json | jq '.[0] | {id, title, description, design, acceptance_criteria, metadata}'
  printf '```\n'
} > <scratch>/<bead>-prompt.md
```

1. `bd show --json` returns a one-element array; `.[0]` is the bead.
2. Start one background agent per bead with the contents of
   `<scratch>/<bead>-prompt.md` as its prompt. Start them together when
   there are several, up to the harness's concurrency limit; the rest start
   as slots free up.
3. The background agent reads the bead from the fenced JSON block, does the
   work against the worktree named in the instruction, and returns its result
   as text. It does not write to beads or ATM.
4. The agent that started it applies the result: closes or reopens beads,
   files findings, closes its own ATM task.

Keep `<scratch>` outside the repository and never commit the prompt files.

## Stacking

Dev and fix work runs in parallel and is stacked when it completes: the dev
completes the work, rebases it onto the current top of its stack, and the
stack writer (lead) links it. Layers therefore stack in the order they
complete, and lead records each bead's actual `layer` when it
is linked; `pr_target` stays the planned lower bound until a dev-fix records the sprint's first layer's branch there. The mechanics are the
`sc-gh-stack` skill (`/sc-gh-stack` in Claude): its `workflow.md`,
`recipe-cut-layer.md` and `recipe-link.md` are plain markdown any agent can
follow.

## Lifecycle

Each assignment is one ATM task and one bead, opened and closed together; the
task id is the bead id. The pairing (ready check, claim, start, close,
refusal) is defined once, in the `atm-bd-orchestration` skill ("Dispatch").

## Dispatch Loop

1. `bd ready -l phase-<x> -n 0` lists the dev, sanity check, QA and finding beads
   whose blockers are closed, highest priority first.
2. Assign each ready bead to the agent the lead picks with the matching assignment
   template, using the bead id as `task_id`.
3. When a task closes, its bead closes with it, except after a failed sanity check or plan review, which leaves the bead open. Run `bd sync`, then `bd ready` again. After a
   green sanity check or a triaged QA, create and wire the QA or finding beads
   first, then run `bd ready`.

## Phase Closure

Closed beads alone do not establish that fixes landed. After integration,
run the phase-end review and finding reconciliation in
[`atm-bd-orchestration/references/post-mortem.md`](../../atm-bd-orchestration/references/post-mortem.md).
Include closed finding beads and verify their resolutions at the pinned
integration head before closing the phase or merging it to the base branch.

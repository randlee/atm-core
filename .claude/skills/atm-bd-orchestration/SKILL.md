---
name: atm-bd-orchestration
version: 0.7.2
description: Bead-driven phase orchestration for the lead. Use when running a phase whose plan is in beads, dispatching from `bd ready` with ATM tasks, and landing it as one gh stack.
requires:
  cli:
    - name: bd
      minimum_version: 1.3.0
    - name: atm
    - name: sc-compose
    - name: jq
    - name: gh
depends_on:
  atm-beads: 0.x
  sc-gh-stack: 0.x
  quality-mgr: 0.x
  ceremony-finding-screen: 0.x
---

# ATM Bead Orchestration

Every orchestration decision is a bead query. The lead never decides what is
ready: `bd ready` answers it, from the dependency edges written at plan time
and at runtime. Agents never decide phase, cursor or done-ness.

The plan is written with the `atm-beads` skill
([`planning.md`](../atm-beads/resources/planning.md),
[`atm-beads-plan-guidelines.md`](../atm-beads/resources/atm-beads-plan-guidelines.md)).
The flow and the dependency edges are in
[`orchestrating.md`](../atm-beads/resources/orchestrating.md); this skill is
the lead's loop and the full set of assignment, close and bead templates.

Never block on bureaucracy. Dev waits only on its prerequisites'
sanity checks; QA, triage and fixes run beside it. 100% of findings are
closed, each with a close reason.

## Step 1 — Verify CLI Installation

Run this before anything else in the skill:

```bash
for c in bd atm sc-compose jq gh; do command -v "$c" >/dev/null && echo "ok $c" || echo "MISSING $c"; done
bd version    # 1.3.0 or newer
gh stack --version   # the gh-stack extension
```

If anything is missing or too old, **read
[`../atm-beads/references/installation-and-troubleshooting.md`](../atm-beads/references/installation-and-troubleshooting.md)
before proceeding.**

## Lead Role

The orchestrator is the **lead**: the identity that dispatches beads, creates
QA beads, links layers into the stack and receives every task close: a close
returns to the task's assigner. The role
can move mid-phase: the outgoing lead sends the incoming lead the open task
ids, open PRs and the stack number, and announces the new lead. In-flight
tasks keep their assigner.

The lead (or its work-orchestrator) is the only stack writer, one at a time (`gh stack link`, `unstack`, `sync`,
`rebase`, `merge`). quality-mgr files the finding beads from QA; the lead
files those from a phase-end review.

The lead keeps BV, the read-only graph analysis it runs at plan review, wave
boundaries, stalls and phase end: [`references/bv.md`](references/bv.md).

Before treating a finding or unresolved decision as a development stop, read
[`blocking-findings-guidelines.md`](blocking-findings-guidelines.md). Scope the
decision and the consequence of choosing wrong. Record decisions for the user
or their delegate; conservative, reversible provisional choices keep independent
work moving. Unresolved decision beads block phase closure, not development.

A serious infrastructure failure an agent cannot fix itself (missing or
invalid API key, out of tokens or quota, provider auth failure, a failing
`gh` or `atm` command and the like) is announced once per cause to the
oversight recipients: ATM's escalation recipients,
`atm escalation list --team "$ATM_TEAM" --json | jq -r '.recipients[]'`, if
empty `atm escalation list --json | jq -r '.recipients[]'`, if both empty the
task assigner, saying in the message that no escalation recipient is set;
`atm send <recipient> --stdin` to each. Once per cause is the cause's workflow
class bead (`workflow-issue-bead.json.j2`, `parent` as in
`examples/workflow-issue-bead-vars.json`, `remedy` = the command that re-tests
the cause): announce only when you create it,
append each later occurrence to it, and close it when the cause clears. When a
backup or fallback exists, use it so work moves forward, never silently: record
each use with its verbatim error in the log or evidence and announce it the same way.

## Repository configuration

Repository values live in one file, `.claude/project/atm-bd-orchestration.yaml`,
which the installer renders from the consuming repository. Scripts read it at
run time through
[`.claude/skills/atm-beads/scripts/repo_config.py`](../atm-beads/scripts/repo_config.py);
a missing file or key is a named error, never a default. Dispatch templates
declare these values as required variables with no defaults, so the lead fills
them from that file (start each vars file from `.claude/skills/atm-beads/scripts/repo_config.py json`); a missing
one fails the render.

| Key | Consumed by |
| --- | --- |
| `bead_prefix` | `sprint_index_common.py` when no root id is given (a root id `<prefix>-phase-<x>` wins) |
| `lead` | `roles.lead` and the install-time lead placeholders |
| `dev_sanity_member` | the `dev-sanity` role (`.claude/skills/atm-beads/scripts/resolve-role dev-sanity`); a team member, which may have no `.claude/agents/<name>.md` |
| `qa_member` | `qa-bead.json.j2` (`qa_member`), a parallel quick fix's QA bead |
| `worktree_base` | sprint bead `worktree` = `<worktree_base>/<branch>` |
| `test_command` | `dev-template`, `fix-assignment`, `dev-fix` |
| `lint_command` | `dev-sanity-template` |
| `integration_branch_pattern` | the root's `integration_branch` (`plan-root.json.j2`), then `review-template` and `plan-review-template` (`integration_branch`) |
| `plans_dir` | `sprint_index_common.py` (`phase_path`), `sprint-report`, `sprint-review`, `plan-review-template` |
| `requirements_globs`, `adr_globs` | `plan-review-template`, `fix-assignment` |
| `policy_path` | `qa-template`, `dev-template`, `fix-assignment`, `schema-reviewer-assignment` |
| `reviewers_round1` | `qa-template` (sprint reviews) |

There is no base-branch key: `validate-plan` reads the plan file from the
`integration_branch` the phase file `.atm-bd/phase-<x>.toml` names.

## Roles

| Role | Who | Prompt |
| --- | --- | --- |
| lead | the appointed lead | this skill |
| dev | frontier / high-value devs; fast agents for important and minor findings | the assignment templates |
| dev-sanity | the member the repository maps to the role, never a dev or fix agent | `.claude/agents/dev-sanity.md` |
| quality-mgr | the long-running QA agent | [`roles/quality-mgr.md`](roles/quality-mgr.md) |
| parallax | optional work-orchestrator: runs the lead's routine orchestration | `.claude/agents/parallax.md` |

This skill names roles, not members or agents. A repository maps a role to
its team-unique member in `roles:` of `.claude/agents/registry.yaml`, and
that member's `[startup.<member>]` prompt in `.atm.toml` names the directive
it runs. Resolve the member with
`.claude/skills/atm-beads/scripts/resolve-role <role>`; it exits 2 when the
role is not mapped.

A long-running agent takes its role at session start, or whenever it is
switched to this workflow, where `<prompt>` is the role's Prompt above
(`.claude/skills/atm-bd-orchestration/roles/quality-mgr.md`,
`.claude/agents/dev-sanity.md`):

```bash
atm send <agent> "Operate under <prompt> for every task until told otherwise. Read it now."
```

Background agents (a subagent or child agent, whichever the harness provides)
receive a bead piped into their prompt as in `orchestrating.md` ("Passing A
Bead To A Background Agent"). They never run `bd` and never write to beads
or ATM.

## Stack Discipline

Every sprint container declares `metadata.pr_target` (the branch of its
nearest `must_follow` prerequisite, or the trunk), which its
poured dev bead takes; it is a lower bound: the PR's actual base (the layer
below it) is `pr_target` or a descendant of it. A fix bead's target is the top of its stack, set at
dispatch. Every PR lands on the top of its stack; there are no forks. A dev or fix branch
is cut from the top of its stack; before dev-complete or fix-complete the dev rebases onto the
stack's current top, opens the PR against it and closes with the `/sc-gh-stack-view` output. The lead
links the PR on top of the phase stack
(`/sc-gh-stack`), before the sanity check. Layer 0 alone cannot form a stack:
its PR waits unlinked on the trunk (sanity accepts it there, and `stack-top`
returns its branch so the next root sprint lands on it), and the lead links
layers 0 and 1 together, `gh stack link --base <trunk> <pr0> <pr1>`, when
layer 1's PR opens; a sanity refusal for no PR, not
stacked or not rebased is the lead's (the stack writer) to fix: it opens,
links or rebases by a new layer as `/sc-gh-stack` prescribes and re-dispatches
the sanity check, without interrupting or messaging the dev.
Rebasing is done before sanity or QA is assigned and is the dev's, at
dev-complete; the lead fixes only what the dev could not, or another stack
problem, before dispatching sanity or QA, in its own worktree, and dispatches
with the new `commit`, `base`, `branch`, `pr_number` and `worktree_path`. A layer that
has passed QA, or has layers above it, is never rebased, and a rebase never
re-dispatches QA or sanity on any other layer. A passed sanity is
frozen and a later change is a new fix bead with its own sanity. Sprint work
does not wait for QA: the next sprint is dispatched as soon as its sanity
blockers PASS. QA and fixes interleave by priority (blocking P1, sprint and
important P2, minor P4).

## Gate Beads

`bd gate` is the strategic way to hold work. A gate holds work to prioritize
other work, waits for critical CI or integration testing (`gh:run` or
`gh:pr`), a timer, or a human decision.
A gate and its edges are created only on the user's explicit instruction for
that gate.

Human gates require explicit user agreement recorded on the gate bead or phase
root; the plan file contains only planned sprint dependencies.

### Parallel Quick Fix

A running sprint sometimes finds a change that other live branches need at
the same time: a shared type or trait signature that parallel sprints all
implement or consume, or a critical bug in code every branch carries. That
change does not go into the finder's layer. Landed there, every other branch
fails until that layer merges, and the cross-fence edits it forces conflict
on every restack and show up as out-of-scope work in that sprint's PR.

1. The finder stops the edit in the sprint worktree and tells the task assigner the
   exact change and the branches it breaks.
2. The lead picks the quick fix's `pr_target`, a lower bound: the lowest
   branch that already holds what the change needs. That is the phase's
   integration branch for a bug in merged code, or the stack layer whose
   types the change uses.
3. The finder cuts `fix/<thing>` from the stack's current top
   (`.claude/skills/atm-bd-orchestration/scripts/assignment-gates.py stack-top --pr-target <pr_target>`) in its own worktree,
   with only the change, the implementors and call sites the compiler
   forces, and one test when it is a bug. The test command passes; push; PR
   against that top: a new layer, never a PR into a lower layer or the
   integration branch. It reports the PR by plain `atm send`.
   When every roster agent is mid-task, the lead runs a background
   developer subagent for this step instead of waiting; the branch,
   scope and test rule are the same.
4. The lead links it on top of the stack and dispatches one QA round on the
   fix PR (`qa-template.xml.j2`, `checked_bead` = the finder's bead, `layer` =
   its layer; its QA bead from `qa-bead.json.j2` with `quick_fix` true, so QA
   skips the sanity-PASS check a quick fix has no sanity for, and `pr_target`,
   which its PR base must descend from). No PR merges without QA. A failed
   quick-fix QA pours nothing and lists its blocking findings in the close's
   `findings_md`; the lead files each as a finding bead (`finding-bead.json.j2`,
   `qa_bead` = the quick-fix QA bead) and dispatches it to an idle roster agent
   (the finder when idle) with `fix-assignment.xml.j2` on the same fix branch when
   nothing is linked above it, else on a new layer cut from the stack's
   current top (Stack Discipline), with no sanity bead; once every finding
   from that QA has closed, one more quick-fix QA bead as in this step. When every roster agent is mid-task,
   the lead assigns the finding's task to itself (`atm task assign
   "$ATM_IDENTITY" --task-id <finding bead> --template fix-assignment.xml.j2
   --vars <vars>`) when its own roster model fits the finding's `difficulty`
   (gate (5)); it runs the template's gate, claim, start and close steps
   itself, acts on the subagent's report for steps c and f1, and a background
   developer subagent does the fix steps (b to e),
   since a background agent never writes to beads or ATM. When the lead's
   model does not fit, the finding stays in `bd ready` for the first fitting
   roster agent to go idle. Every other branch picks the fix up by rebasing onto
   its stack's current top at its dev-complete; the lead tells its owner the
   base moved.
5. The lead records the fix branch and PR in the finder's bead notes and in
   the notes of every bead whose fence it touched. The finder's sprint task
   stays open and continues on the rebased layer.

Once the fix is in the base, it leaves every rebased branch's PR diff, so CI
and QA on those PRs never see it, and no sprint carries another sprint's
edits. It also means one copy of the missing code: without it each blocked
sprint writes its own version of the change; the copies conflict at restack and
the designs drift apart.

## Plan Gate

No dev bead is dispatched until the plan passes review.

1. Create the plan-review bead right after the import and the pour (the
   import procedures do this as their next step), so that every sprint
   container blocks on it. For sprints imported into a running phase, use
   `<root>-plan-qa-<n>` (the next free number) and block only the new sprint
   containers. It gets no assignee; dispatch sets it:

   ```bash
   bd create --id <root>-plan-qa --type task --parent <root> \
     -l phase-<x>,stage:plan-review --title "phase-<x>: plan review"
   bd dep add <sprint container> <root>-plan-qa    # once per sprint container
   ```

2. Generate the initial phase diagram before review:
   `.claude/skills/sprint-review/scripts/sprint-review --root <root>`
   writes the HTML locally; it never commits or pushes.
   The phase integration branch must contain the committed/pushed plan file
   `<plans_dir>/phase-<x>.jsonl` and phase file `.atm-bd/phase-<x>.toml`. Do not open the
   diagram unless `--view` was requested and Wyvern is available.
   Then run `.claude/skills/atm-beads/scripts/validate-plan --phase <x>`
   from the repository root; its header lists what it checks. Exit 0 or stop.

3. Dispatch it with
   [`plan-review-template.xml.j2`](templates/plan-review-template.xml.j2).
   - PASS closes the bead and releases the root sprints. With minor
     findings, it is handed to you open; you fix them and close it.
   - FAIL leaves it open. The author fixes the beads with `bd update`; you
     assign the next round (`round` + 1, `carry_forward` = the open
     finding lines) with the same task id. That round runs only each carried
     finding's filing reviewer.
   - Plan review is capped at three rounds.

Requirements and ADRs are the tight part of the gate. Every dev bead lists
its governing ids in `metadata.requirements` and `metadata.adrs`, or exactly
`["NONE"]`. quality-mgr rejects the plan as blocking when a list is missing,
names an id that does not exist (except as
[New Ids](../atm-beads/resources/planning.md#new-ids) allows), names an id that does not govern the work,
or omits one the work touches. The dev reads them before coding, and QA
checks the change against them.

## Loop

```bash
bd ready -l phase-<x> -n 0 --json
```

`bd ready` lists every bead whose blockers are closed, highest priority
first: dev beads whose prerequisites' sanity checks passed, sanity checks
whose dev or fix bead closed, QA beads whose sanity check passed, fix beads and
open findings. Run it after every task close, before any other work, and dispatch every
ready bead. Assign a bead only while `bd ready` lists it. Order and hold work only with bead edges, gates and `atm task move`;
never tell an agent not to run a task in its queue. The lead may step in at critical points, preferably through a background developer subagent (Parallel Quick Fix step 3); lead work is never part of the original plan.
While a workflow outage class bead is open, re-test its cause on each pass
(Jev: `python3 .claude/skills/atm-bd-orchestration/scripts/jev_client.py --startup`; any other cause: the command
in the bead's `remedy`, its `design`) and on success close it (`bd close <bead> --reason
"re-test PASS"`), which releases the re-dispatches held on it. A passing probe
does not close a Jev class bead a JEV child opened while a sanity task is ready
or open; dev-sanity closes it on its next passing JEV child
(`.claude/agents/dev-sanity.md` Startup). When none is, the passing probe
closes it too.
For each ready bead:

| Ready bead | Template | To |
| --- | --- | --- |
| plan review (`stage:plan-review`) | [`plan-review-template.xml.j2`](templates/plan-review-template.xml.j2) | quality-mgr |
| dev (`stage:dev`) | [`dev-template.xml.j2`](templates/dev-template.xml.j2) | the member the lead picks for its `difficulty` |
| sanity check (`stage:dev-sanity`) | [`dev-sanity-template.xml.j2`](templates/dev-sanity-template.xml.j2) | `.claude/skills/atm-beads/scripts/resolve-role dev-sanity` |
| QA (`stage:qa`) | [`qa-template.xml.j2`](templates/qa-template.xml.j2) | quality-mgr |
| fix (`stage:fix`) | [`fix-assignment.xml.j2`](templates/fix-assignment.xml.j2) | the member the lead picks for its `difficulty` |
| finding from a quick-fix QA (its `discovered-from` QA bead has `metadata.quick_fix` true) | [`fix-assignment.xml.j2`](templates/fix-assignment.xml.j2), no sanity bead; once every finding from that failed QA has closed, one more quick-fix QA bead (Parallel Quick Fix step 4) | an idle dev whose model fits its `difficulty`, by priority |
| important or minor finding (`stage:finding`, no `metadata.sanity_finding`, not from a quick-fix QA) | [`fix-assignment.xml.j2`](templates/fix-assignment.xml.j2), in the same step as one `bd import` of its sanity bead (`atm-beads` [`dev-sanity-bead.json.j2`](../atm-beads/templates/dev-sanity-bead.json.j2), `dev_bead` = the finding) and its QA bead ([`qa-bead.json.j2`](templates/qa-bead.json.j2), `checked_bead` = the finding, `blocked_by` = the sanity bead), each with `parent` = the finding's parent; a minor finding left in the backlog gets neither until it is assigned | an idle dev, by priority |
| sanity finding (`stage:finding` with `metadata.sanity_finding`) | its checked bead's [`dev-fix.xml.j2`](templates/dev-fix.xml.j2), once per checked bead | the checked bead's assignee |
| review (`stage:review`) | [`review-template.xml.j2`](templates/review-template.xml.j2) | the phase-end reviewer |

Then, on each task close:

| Close | Lead does |
| --- | --- |
| plan-review PASS | nothing when the bead closed: the root sprints are now ready. With minor findings the bead is assigned to you still open: fix each listed bead with `bd update`, then `bd close <root>-plan-qa --reason "minor fixes applied"` |
| plan-review FAIL | have the author fix the listed beads, run `validate-plan` again, then dispatch the next round |
| dev-complete | verify the dev's PR and link it on top of the phase stack, fixing any stack problem yourself (Stack Discipline), record it on the checked bead (`bd update <bead> --append-notes "layer PR #<n> <url>"`, from which a later `layer_prs` is read), then assign the now-ready sanity check; after a dev-fix with `layer_prs` = the sprint's layer PRs, its first layer's (`gh pr view <metadata.pr_target> --json number`) first and the checked PR last |
| sanity check PASS | verify the PR base is its `pr_target` or a descendant of it; the group's poured QA bead is now ready: dispatch it with `checked_bead` = its `metadata.checked_bead` and `sprint_bead` = its `metadata.sprint_bead`, and `base` = the PR base; for a sprint that had a dev-fix also `layer_prs` = the sprint's layer PRs, its first layer's (`gh pr view <metadata.pr_target> --json number`) first and the checked PR last, so the QA diff is the sprint's own layer ranges and never another sprint's layer between them. For a fix bead's QA, also `carry_forward` = the fix bead (it carries its own requirements and ADRs) and `round` = the fix bead's `metadata.round` + 1. For an important or minor finding bead, the group's QA bead is now ready: dispatch it with `checked_bead` and `carry_forward` = the finding |
| sanity check FAIL | one coordinator runs LLM/JEV with shared lint, then `sanity-selected` chooses whole replies per deliverable and alone creates finding children. Sanity answers only whether a numbered deliverable is written; requirements and quality are QA. The sanity member creates one child finding bead for every selected undone deliverable under `<checked bead>` at `clamp(parent priority - 1, P1, P4)`, so it ranks ahead of the parent's peers, never one for lint; it does not edit the parent. It copies phase/sprint/stack/layer provenance and uses only `phase-<phase>`, `stage:finding`, and `stack:<stack>` labels. The hierarchy is the parent closure gate (`bd` rejects parent-to-child `blocks` edges); it adds `blocks` edges only between those new beads, where one fix depends on another. Each child stores the exact structured report data. dev-sanity has reopened the checked bead; on a first FAIL the lead assigns it with [`dev-fix.xml.j2`](templates/dev-fix.xml.j2) on a new layer: cut its branch and worktree from the current top of the stack, record the sprint's first layer's branch as the checked bead's `metadata.pr_target` (`bd update <bead> --set-metadata pr_target=<branch>`; a later dev-fix keeps it) and pass it as `pr_target`; never rebase or re-target a linked layer. When the children are excessive, the lead first verifies against the branch that each one's work is really not done and closes, with a reason, any that judges correctness or quality (that is QA). The lead may overrule, amend, split, or reassign children, but does not recreate them. After the second FAIL for the same checked bead, before dispatching a fix the lead diffs flagged files versus the last PASS, checks the branch base for foreign commits, then rules; report `SANITY.ROUND_CAP` with undone deliverable numbers and do not run a third round without that ruling. The parent cannot close until every child closes. This applies to a finding bead too: its task id is the finding, and it closes with `dev-complete.md.j2` |
| qa-complete | nothing: quality-mgr poured a fix group under the sprint per blocking finding (round n+1 for a failed fix verification) and filed the rest as finding beads before it closed the QA bead; the fix beads are now ready. A failed quick-fix QA (`metadata.quick_fix`) poured none: file and dispatch its blocking findings (Parallel Quick Fix step 4). When no step is open under the sprint, close it (below) |
| fix-complete (`fixed`) | verify the dev's PR and link it on top of the phase stack, fixing any stack problem yourself (Stack Discipline), and record it on the fix bead (`bd update <bead> --append-notes "layer PR #<n> <url>"`). For a poured fix bead or an important or minor finding bead, nothing more: the group's sanity check is now ready. For a quick-fix finding, once every finding from that failed QA has closed, one more quick-fix QA bead (Parallel Quick Fix step 4) |
| review-complete | file each finding with `finding-bead.json.j2` (`qa_bead` = the review bead) |
| task-refused | read the reason and the bead state (`open`, or `blocked-failed` for a dev bead that declared failure). Reassign it only once `bd ready` lists the bead (a blocker refusal: as the not-ready or blocker row), split it, or close the bead yourself with `bd close <bead> --force --reason "<why>"`. A `blocked` bead is never in `bd ready`: run `bd update <bead> --status open --assignee <new agent>` before you re-dispatch it. A sanity refusal for no PR, not stacked or not rebased is yours as stack writer (Stack Discipline). A cannot-run caused by an announced outage (its workflow class bead is open) is not re-dispatched until the cause clears and that bead closes; never force-close the bead meanwhile. `REVIEW_PENDING_JEV`: file each code finding in its notes with `finding-bead.json.j2` (`qa_bead` = the review bead) and re-dispatch the review only after the Jev outage clears, with the prior `post_mortem_jev` run IDs in its notes so they are reused |
| fix-complete (`not_reproducible`) | no commit, no sanity check; the finding is closed. For a poured fix bead or an important or minor finding bead, close its group's sanity bead, then its QA bead, each with `bd close <bead> --reason "not_reproducible: <fix bead>"`. For a quick-fix finding, as for `fixed` |
| assignee stopped on an active task (it acts as if pair-programming: states what it will do next and waits for a user who does not exist; a turn ending on a promise of future work, status-only replies to reminders, `lead_notified` mail, a bead-queues `task_stalled` row) | the lead gets it running again on the first sign, never waiting for the reminder budget: `atm send <assignee>` "Create a checklist for all deliverables for this task, outside the worktree's tracked tree (an untracked file in the worktree fails the dev-sanity clean-worktree check). Go through the checklist one item at a time and complete the task, marking the task complete AFTER you verify that the task is done and tests pass. Follow the task complete process, closing bd and notifying the task assigner when all deliverables are complete." |
| assignee silent past the re-nudge | announce it as in Lead Role, then `bd update <bead> --status open --assignee <new agent>` and re-assign the same task id with the same template and vars: `atm task assign <new agent> --task-id <same id> --template <same> --vars <same>` |
| not-ready or blocker refusal | the task is closed `refused` and the bead is open. Fix the cause it names (usually a blocker still open; a blocker that is not a bead, such as an unfiled fix, gets one first: [`finding-bead.json.j2`](templates/finding-bead.json.j2) or a workflow class bead, whichever fits) and add the dependency it recommends (`bd dep add <bead> --blocked-by <blocker>`, or the right one, sprint beads included; never a replan) so `bd ready` holds the bead until the blocker closes. When bd refuses that edge (it keeps one edge type per bead pair, so a QA bead that already `validates` its checked bead takes no other, and it refuses a parent-child edge), add the edge to an open bead the blocker's work goes through instead (`bd dep add <bead> --blocked-by <it>`, unless it already blocks the bead): for a QA bead whose checked bead changes after its sanity PASS, the change is a new fix bead with its own sanity bead (a passed sanity is frozen, never reopened), and that sanity bead holds the QA bead; never remove an edge; re-assign the same bead (same task id, template and vars) only once `bd ready` lists it. A blocker refusal is never answered with "wait". If the work is no longer wanted, close the bead with a reason instead |

Re-run `bd ready` after every close. Never cache the ready list. The open
phase root also appears in it; it is never dispatched.
On each Loop pass also run `.claude/skills/atm-bd-orchestration/scripts/bead-queues --phase <x>` and act on every row it reports.

Close a sprint container when every child is closed (`bd children <sprint>
--json | jq -e 'all(.[]; .status == "closed")'`): `bd close <sprint> --reason
"<n> fix groups closed; QA PASS"`. bd refuses the close while a child is open.

After every bead write, run `.claude/skills/atm-beads/scripts/validate-plan --phase <x>`. On any problem,
stop dispatching and report it to the user; never repair the graph
(`bd dep`, `--parent`) beyond adding a dependency discovered in motion. A DAG problem is fixed by replanning: edit
the plan file in a `/sc-git-worktree` branch off the root's
`integration_branch` and merge the plan PR into it. While a phase is in motion its sprint set is frozen; the lead only adds
dependencies: to fix beads created during the phase, and any dependency
discovered in motion (a blocker refusal names it), sprint beads included,
with `bd dep add <bead> --blocked-by <blocker>`. A discovered dependency
is never a replan; a planned edge is never removed.
Verify branches read-only (`git -C <worktree> log`,
`git diff`, `gh pr view`); never run a state-changing command in an
assignee's worktree.

When every sprint and finding bead is closed
(`bd list -l phase-<x> --status open,in_progress,blocked -n 0 --json` lists only
the phase root), land the phase stack on the root's `integration_branch`, then run the
phase-end review below on that integration commit. A closed finding is not
proof that its fix landed. Close the phase root and merge the phase to the base branch only
after the integration post-mortem passes at the final integration head; then
run `bd sync`.

### Phase-End Review

Quality-mgr runs the required JEV screening and investigated post-mortem.
Read [references/post-mortem.md](references/post-mortem.md). The phase-end
review includes a reconciliation of every finding bead, including closed
ones, against the final integration source. Assign `branch` = the root's
`integration_branch` and `commit` = its fetched, pinned head; reviewing a
stack tip alone does not satisfy this gate.

After the sprint/fix stack has landed on the integration branch, create the
review bead and dispatch it with `review-template.xml.j2`. After corrections,
reuse its carried verification scope at the new integration head:

```bash
bd create --id <root>-review --type task --parent <root> \
  -l phase-<x>,stage:review --assignee <reviewer> \
  --title "phase-<x>: phase-end review"
```

On review-complete, file each finding with `finding-bead.json.j2`, using:

- `qa_bead` = the review bead;
- `sprint` and `found_on_layer` from the metadata of the dev bead whose code
  it cites (the phase root when it spans sprints) (`phase-end` and
  the top layer when it is the root);
- `found_at_commit` = the reviewed commit;
- `screen` = `keep`, unless you ran `ceremony-finding-screen` over them;
- `finding_ref` = the reviewer's numbering (`R-1`, `R-2`, …);
- `sprint_bead`, `requirements` and `adrs` = the cited dev bead's id and
  lists. For a finding that spans sprints, use the root as `sprint_bead`. For
  `requirements` and for `adrs` separately, take the union of the real ids
  of the sprints it touches, dropping every `NONE` and duplicate. Use
  exactly `["NONE"]` only when that union is empty.

Fix them as for any finding and land their fixes on the integration branch.
Have the filing reviewers verify the carried findings there, then refresh
the post-mortem inventory and integration evidence at the new head. Do not
restart a full code sweep for this verification pass. The completion report
must say `integration_review_passed`; missing evidence, unresolved findings,
or a different integration head leave phase closure pending.

## Sync

Every agent on this host writes to the same shared Dolt server, so a claim
or close is visible to everyone at once. `bd sync` (pull, conflict check,
blocked-flag repair, push) exists only for the Dolt remote: the off-host
copy and any other machine. The lead owns it and runs it:

- right after a plan import;
- after handling each close in the Loop, before the next `bd ready`;
- at phase end, before landing the stack.

Assignees never push. If every agent pushed after every write, they would
race each other for the remote (exit 3). Do not dispatch while `bd sync`
fails:

| Exit | Meaning | Lead does |
| --- | --- | --- |
| 0 | synced | continue |
| 1 | transport, auth or storage error | report it to the user; keep working locally and retry on the next close |
| 2 | merge conflict, nothing pushed | stop dispatching; report it to the user, who resolves it by hand |
| 3 | push race or another writer mid-write | retry on the next close |
| 4 | a stuck dirty working set | stop dispatching; report it to the user |

An agent on another machine has its own database. It must run `bd sync`
before its ready check and after its close. No such agent exists today.

## Dispatch

Every assignment is one ATM task and one bead, and the task id **is** the
bead id:

```bash
atm task assign <agent> --task-id <bead> \
  --template .claude/skills/atm-bd-orchestration/templates/<template> \
  --vars <scratch>/<bead>-vars.json
```

- Before the first dispatch of a phase, create the root's `integration_branch`
  from the base branch and push it.
- For a dev or finding bead, create its branch and worktree from the current
  top of its stack (its `pr_target` or a descendant of it), passed as the
  assignment's `pr_target`: `git fetch origin && git worktree add -b <branch> <worktree>
  origin/<top>`. A `dev-fix.xml.j2` assignment's layer is cut the same way; its
  `pr_target` is the sprint's first layer's branch.
- Set the bead's assignee to the recipient first:
  `bd update <bead> --assignee <agent>`. For a role, the recipient is
  `.claude/skills/atm-beads/scripts/resolve-role <role>`. A claim fails when the bead is
  assigned to anyone else.
- Build vars from the template's `required_variables`, with `task_id` = the
  bead id; the bead supplies most of the rest
  (`bd show <bead> --json | jq '.[0].metadata'`). Keep vars files outside the
  repository.
- Preview with `atm compose --template <template> --vars <file>`. Never
  render and paste a body.

The assignee pairs every ATM step with its bead step:

| Step | Bead | ATM |
| --- | --- | --- |
| ready check | `bd ready -n 0 --json` lists the bead | |
| start | `bd update <bead> --claim` | then `atm task start <bead> "<one line>"` |
| done | `bd close <bead> --reason "<why>"` | with `atm task close <bead> completed --template <complete> --vars <file>` |
| refused | bead returned open with no assignee (always for a blocker), or `blocked` with a `DEV_CANNOT_COMPLETE` note for a dev bead that cannot be completed at all | with `atm task close <bead> refused --template task-refused.md.j2 --vars <file>` |

- `bd update --claim` succeeds on a blocked bead, so the ready check comes
  first. A bead that is not ready is neither claimed nor started: the
  assignee finds the root cause (`bd blocked --json`, then `bd show` on each
  blocker) and refuses the task, naming the blocker and the dependency to add.
  A blocker met mid-task is the same: the bead goes back open, the task closes
  `refused`, and nobody waits on an active task.
- A failed sanity check or plan review leaves its bead open, because closing
  it would release the beads it blocks (see
  `.claude/agents/dev-sanity.md` and "Plan Gate").
- Every task closes with a template. A push or progress report closes
  nothing.

## ATM Limits Today

These split an ATM task from its bead. The interim rule is in the
templates. Each is a requirement for the planned combined `atm bd claim` /
`atm bd close`.

| Gap | Interim rule | `atm bd` requirement |
| --- | --- | --- |
| one active task per agent | ATM nudges one active task at a time; dev-sanity and quality-mgr execute every open task at once (`atm task start` for the active one, `bd update --claim` for all) and close a queued task without starting it | several active tasks for a coordinator role |
| actor identity must be explicit | pass `--actor "$ATM_IDENTITY"` on every `bd` write | the actor is always `ATM_IDENTITY` |
| claim fails when the bead is assigned to someone else | lead sets the assignee before `atm task assign`; returned beads clear it | claim reassigns the bead to the task's assignee |
| a bead can close with no task (ceremony finding) and a task can close with no bead change (cancel) | allowed; both carry a reason | a bead-only close and a task-only cancel |

## Templates

Copied from codex-orchestration and updated for beads, plus the templates
this workflow adds. `sprint-plan.md.j2` is not carried over: the plan is
written to beads with the `atm-beads` templates.

| Template | Kind | Used by |
| --- | --- | --- |
| `plan-review-template.xml.j2` | assignment | lead → quality-mgr, the plan-review bead |
| `plan-review-complete.md.j2` | close | quality-mgr, PASS or FAIL |
| `dev-template.xml.j2` | assignment | lead → dev, a planned dev bead |
| `dev-fix.xml.j2` | assignment | lead → dev, a dev bead reopened by a failed sanity check |
| `dev-complete.md.j2` | close | dev, for `dev-template` and `dev-fix` |
| `dev-sanity-template.xml.j2` | assignment | lead → dev-sanity member |
| `dev-sanity-assignment.json.j2` | fenced JSON | dev-sanity (`sanity-split`) → one `sc-sanity-llm` or `sc-sanity-jev` subagent per numbered deliverable |
| `dev-sanity-complete.md.j2` | close | dev-sanity member, PASS or FAIL |
| `qa-template.xml.j2` | assignment | lead → quality-mgr |
| `qa-complete.md.j2` | close | quality-mgr, and its PR comment |
| `fix-assignment.xml.j2` | assignment | lead → dev, one poured fix bead or important or minor finding bead |
| `fix-complete.md.j2` | close | dev, for `fix-assignment` |
| `review-template.xml.j2` | assignment | lead → phase-end reviewer |
| `review-complete.md.j2` | close | phase-end reviewer |
| `task-refused.md.j2` | close | anyone who cannot do the whole assignment |
| `req-qa`, `arch-qa`, `ruthless-boundary-qa`, `flaky-test-qa`, `schema-reviewer`, `plan-scope-reviewer` `-assignment.json.j2` | fenced JSON | quality-mgr → its background reviewers; `plan-scope-reviewer` on plan QA-1, then only for its carried findings, locked to each finding's original acceptance criterion |
| `qa-bead.json.j2` | bead | lead, when it assigns an important or minor finding bead (with its sanity bead) or for a parallel quick fix (`quick_fix` true; sprint and fix QA beads are poured) |
| `finding-bead.json.j2` | bead | quality-mgr (QA, important and minor) and lead (review), one per finding |
| `workflow-issue-bead.json.j2` | bead | anyone, a workflow class bead for a recurring failure signature |

Bead templates render with `sc-compose render --file <t> --var-file <v>
--strict`, then `jq -c .` into a JSONL file for `bd import`. Examples of every
template's vars are in [`examples/`](examples/).

## Priority

`bd ready` sorts by priority, so priority is the queue order. The finding
template derives it from severity: blocking P1, planned dev P2, important P2,
minor P4. The lead still picks the assignee; the usual case is a frontier dev
for blocking and dev work and a fast agent for important and minor.

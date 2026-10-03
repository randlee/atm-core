---
name: atm-bd-orchestration
version: 0.3.7
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
QA beads, links layers into the stack and receives every task close. Templates
address it through the `lead` variable (default `team-lead`) and copy
reports to `cc` (default `team-lead`; empty switches copies off). The role
can move mid-phase: the outgoing lead sends the incoming lead the open task
ids, open PRs and the stack number, and announces the new lead. In-flight
tasks keep their assigner.

The lead is the only stack writer (`gh stack link`, `unstack`, `sync`,
`rebase`, `merge`). quality-mgr files the finding beads from QA; the lead
files those from a phase-end review.

Before treating a finding or unresolved decision as a development stop, read
[`blocking-findings-guidelines.md`](blocking-findings-guidelines.md). Scope the
decision and the consequence of choosing wrong. Record decisions for the user
or omega-prime; conservative, reversible provisional choices keep independent
work moving. Unresolved decision beads block phase closure, not development.

## Roles

| Role | Who | Prompt |
| --- | --- | --- |
| lead | the appointed lead | this skill |
| dev | frontier / high-value devs; fast agents for important and minor findings | the assignment templates |
| dev-sanity | the member the repository maps to the role, never a dev or fix agent | [`roles/dev-sanity.md`](roles/dev-sanity.md) |
| quality-mgr | the long-running QA agent | [`roles/quality-mgr.md`](roles/quality-mgr.md) |

This skill names roles, not members or agents. A repository maps a role to
its team-unique member in `roles:` of `.claude/agents/registry.yaml`, and
that member's `[startup.<member>]` prompt in `.atm.toml` names the directive
it runs. Resolve the member with
`.claude/skills/atm-beads/scripts/resolve-role <role>`; it exits 2 when the
role is not mapped.

A long-running agent takes its role at session start, or whenever it is
switched to this workflow:

```bash
atm send <agent> "Operate under .claude/skills/atm-bd-orchestration/roles/<role>.md for every task until told otherwise. Read it now."
```

Background agents (a subagent or child agent, whichever the harness provides)
receive a bead piped into their prompt as in `orchestrating.md` ("Passing A
Bead To A Background Agent"). They never run `bd` and never write to beads
or ATM.

## Stack Discipline

Every bead declares its immutable `metadata.pr_target`. A dev or fix branch
is cut from `origin/<pr_target>` and rebases only onto that branch. The lead
opens the PR against that same target after sanity PASS; a passed sanity is
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
root; the canonical `sprints.jsonl` contains only planned sprint dependencies.

### Parallel Quick Fix

A running sprint sometimes finds a change that other live branches need at
the same time: a shared type or trait signature that parallel sprints all
implement or consume, or a critical bug in code every branch carries. That
change does not go into the finder's layer. Landed there, every other branch
fails until that layer merges, and the cross-fence edits it forces conflict
on every restack and show up as out-of-scope work in that sprint's PR.

1. The finder stops the edit in the sprint worktree and tells the lead the
   exact change and the branches it breaks.
2. The lead picks the base: the lowest branch that already holds what the
   change needs. That is `integrate/phase-<x>` for a bug in merged code, or
   the stack layer whose types the change uses.
3. The finder cuts `fix/<thing>` from `origin/<base>` in its own worktree,
   with only the change, the implementors and call sites the compiler
   forces, and one test when it is a bug. The test command passes; push; PR
   into `<base>`.
   When every roster agent is mid-task, the lead runs a background
   `rust-developer` subagent for this step instead of waiting; the branch,
   scope and test rule are the same.
4. The lead dispatches one QA round on the fix PR (`qa-template.xml.j2`,
   `checked_bead` = the finder's bead, `layer` = the base) and merges when
   it passes; no PR into the integration branch or a stack layer merges
   without QA. Every branch whose `pr_target` is the base rebases onto
   `origin/<pr_target>` at its next push; the lead tells its owner the base moved.
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

1. Create the plan-review bead right after the import (the import
   procedures do this as their next step), so that it blocks every root sprint (every dev
   bead with no sanity check blocker). For sprints imported into a running
   phase, use `<root>-plan-qa-<n>` (the next free number) and block only the
   new dev beads, including any that already have sanity check blockers:

   ```bash
   bd create --id <root>-plan-qa --type task --parent <root> \
     -l phase-<x>,stage:plan-review --assignee quality-mgr \
     --title "phase-<x>: plan review" --deps blocks:<root sprint>,blocks:<root sprint>
   ```

2. Generate and publish the initial phase diagram before review:
   `.claude/skills/sprint-review/scripts/sprint-review --root <root>`.
   The phase integration branch must contain the committed/pushed
   `docs/plans/phase-<x>/sprints.jsonl` canonical dependency tuples and
   `docs/plans/phase-<x>/phase-<x>-dag.html` with embedded SVG. Do not open the
   diagram unless `--view` was requested and Wyvern is available.
   Then run `.claude/skills/atm-beads/scripts/validate-plan --root <root>`
   from the repository root. It checks beads, REQ/ADR references, ATM members,
   index membership, and the published artifacts. Exit 0 or stop.

3. Dispatch it with
   [`plan-review-template.xml.j2`](templates/plan-review-template.xml.j2).
   - PASS closes the bead and releases the root sprints. With minor
     findings, it is handed to you open; you fix them and close it.
   - FAIL leaves it open. The author fixes the beads with `bd update`; you
     assign the next round (`round` + 1, `carry_forward` = the open
     findings) with the same task id.
   - Plan review is capped at three rounds, as in `quality-mgr.md`.

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
whose dev bead closed, QA beads and open findings. For each ready bead:

| Ready bead | Template | To |
| --- | --- | --- |
| plan review (`stage:plan-review`) | [`plan-review-template.xml.j2`](templates/plan-review-template.xml.j2) | quality-mgr |
| dev (`stage:dev`) | [`dev-template.xml.j2`](templates/dev-template.xml.j2) | its assignee |
| sanity check (`stage:dev-sanity`) | [`dev-sanity-template.xml.j2`](templates/dev-sanity-template.xml.j2) | `resolve-role dev-sanity` |
| QA (`stage:qa`) | [`qa-template.xml.j2`](templates/qa-template.xml.j2) | quality-mgr |
| finding (`stage:finding`) | [`fix-assignment.xml.j2`](templates/fix-assignment.xml.j2) | the member the lead picks |
| review (`stage:review`) | [`review-template.xml.j2`](templates/review-template.xml.j2) | the phase-end reviewer |

Then, on each task close:

| Close | Lead does |
| --- | --- |
| plan-review PASS | nothing when the bead closed: the root sprints are now ready. With minor findings the bead is assigned to you still open: fix each listed bead with `bd update`, then `bd close <root>-plan-qa --reason "minor fixes applied"` |
| plan-review FAIL | have the author fix the listed beads, run `validate-plan` again, then dispatch the next round |
| dev-complete | nothing: the sanity check is now ready |
| sanity check PASS | verify the branch base is its declared `pr_target`, then create and dispatch the QA bead from [`qa-bead.json.j2`](templates/qa-bead.json.j2) (a child of the checked bead). For a finding, the QA dispatch sets `checked_bead` = the finding (it carries its own requirements and ADRs), `sprint_bead` = its `metadata.sprint_bead`, `carry_forward` = the finding id, and `round` = the `metadata.round` of the QA bead it was `discovered-from`, + 1, or 1 when it came from the phase-end review |
| sanity check FAIL | one coordinator runs assignment-selected `both` (default), `llm`, or `jev` with shared lint and separate logged reviewer results; both uses LLM as operational authority and JEV as comparison-only; only the operational reviewer creates finding children; sanity answers only whether a numbered deliverable is written; requirements and quality are QA. The sanity member creates one child finding bead for every undone deliverable under `<checked bead>` at `clamp(parent priority - 1, P1, P4)`, so it ranks ahead of the parent's peers, never one for lint; it does not edit the parent. It copies phase/sprint/stack/layer provenance and uses only `phase-<phase>`, `stage:finding`, and `stack:<stack>` labels. The hierarchy is the parent closure gate (`bd` rejects parent-to-child `blocks` edges); it adds `blocks` edges only between those new beads, where one fix depends on another. Each child stores the exact structured report data. On a first FAIL the lead reopens the checked bead and assigns it with [`dev-fix.xml.j2`](templates/dev-fix.xml.j2). When the children are excessive, the lead first verifies against the branch that each one's work is really not done and closes, with a reason, any that judges correctness or quality (that is QA). The lead may overrule, amend, split, or reassign children, but does not recreate them. After the second FAIL for the same checked bead, before dispatching a fix the lead diffs flagged files versus the last PASS, checks the branch base for foreign commits, then rules; report `SANITY.ROUND_CAP` with undone deliverable numbers and do not run a third round without that ruling. The parent cannot close until every child closes. This applies to a finding bead too: its task id is the finding, and it closes with `dev-complete.md.j2` |
| qa-complete | quality-mgr files finding beads, applies the ceremony screen, and reports the verdict. |
| fix-complete (`fixed`) | create the fix's sanity check bead (`atm-beads` [`dev-sanity-bead.json.j2`](../atm-beads/templates/dev-sanity-bead.json.j2), `dev_bead` and `parent` = the finding) |
| review-complete | file each finding with `finding-bead.json.j2` (`qa_bead` = the review bead) |
| task-refused | read the reason and the bead state (`open`, or `blocked-failed` for a dev bead that declared failure). Reassign it, split it, or close the bead yourself with `bd close <bead> --force --reason "<why>"`. A `blocked` bead is never in `bd ready`: run `bd update <bead> --status open --assignee <new agent>` before you re-dispatch it |
| fix-complete (`not_reproducible`) | nothing: no commit, no sanity check; the finding is closed |
| not-ready report | the task is still open and queued. Fix the cause it names (usually a blocker still open) and tell the assignee to run the ready check again. If the work is no longer wanted, close the task `cancelled` with `task-refused.md.j2` (`bead_state` open) |

Re-run `bd ready` after every close. Never cache the ready list. The open
phase root also appears in it; it is never dispatched.

After every bead write, run `validate-plan --root <root>`. On any problem,
stop dispatching and report it to the user; never repair the graph
(`bd dep`, `--parent`). A DAG problem is fixed by replanning: edit
`sprints.jsonl` in a `/sc-git-worktree` branch off `develop` and merge the
plan PR. While a phase is in motion its sprint DAG is frozen; only
dependencies to fix beads created during the phase are added or changed.
Verify branches read-only (`git -C <worktree> log`,
`git diff`, `gh pr view`); never run a state-changing command in an
assignee's worktree.

When every sprint and finding bead is closed
(`bd list -l phase-<x> --status open,in_progress,blocked -n 0 --json` lists only
the phase root), land the phase stack on `integrate/phase-<x>`, then run the
phase-end review below on that integration commit. A closed finding is not
proof that its fix landed. Close the phase root and merge to `develop` only
after the integration post-mortem passes at the final integration head; then
run `bd sync`.

### Phase-End Review

Quality-mgr runs the required JEV screening and investigated post-mortem.
Read [references/post-mortem.md](references/post-mortem.md). The phase-end
review includes a reconciliation of every finding bead, including closed
ones, against the final integration source. Assign `branch` =
`integrate/phase-<x>` and `commit` = its fetched, pinned head; reviewing a
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

- Before the first dispatch of a phase, create `integrate/phase-<x>` from
  `develop` and push it.
- For a dev or finding bead, create its branch and worktree from its declared
  target: `git fetch origin && git worktree add -b <branch> <worktree>
  origin/<pr_target>`.
- Set the bead's assignee to the recipient first:
  `bd update <bead> --assignee <agent>`. For a role, the recipient is
  `resolve-role <role>` (a sanity check bead is already assigned to it). A claim fails when the bead is
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
| refused | bead returned open with no assignee, or `blocked` with a `failed:` note for a dev bead | with `atm task close <bead> refused --template task-refused.md.j2 --vars <file>` |

- `bd update --claim` succeeds on a blocked bead, so the ready check comes
  first. A bead that is not ready is neither claimed nor started: the
  assignee finds the root cause (`bd blocked --json`, then `bd show` on each
  blocker) and reports it to the lead.
- A failed sanity check or plan review leaves its bead open, because closing
  it would release the beads it blocks (see
  [`roles/dev-sanity.md`](roles/dev-sanity.md) and "Plan Gate").
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
| only the original assigner can re-dispatch a closed task id | the lead that dispatched a bead re-dispatches it; after a lead handover, the outgoing lead re-dispatches the beads it had already dispatched | the current lead may take over closed tasks |
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
| `dev-sanity-complete.md.j2` | close | dev-sanity member, PASS or FAIL |
| `qa-template.xml.j2` | assignment | lead → quality-mgr |
| `qa-complete.md.j2` | close | quality-mgr, and its PR comment |
| `fix-assignment.xml.j2` | assignment | lead → dev, one finding bead |
| `fix-complete.md.j2` | close | dev, for `fix-assignment` |
| `review-template.xml.j2` | assignment | lead → phase-end reviewer |
| `review-complete.md.j2` | close | phase-end reviewer |
| `task-refused.md.j2` | close | anyone who cannot do the whole assignment |
| `req-qa`, `arch-qa`, `ruthless-boundary-qa`, `flaky-test-qa`, `schema-reviewer`, `plan-scope-reviewer` `-assignment.json.j2` | fenced JSON | quality-mgr → its background reviewers; `plan-scope-reviewer` every plan-review round |
| `qa-bead.json.j2` | bead | lead, after a green sanity check |
| `finding-bead.json.j2` | bead | quality-mgr (QA) and lead (review), one per finding |

Bead templates render with `sc-compose render --file <t> --var-file <v>
--strict`, then `jq -c .` into a JSONL file for `bd import`. Examples of every
template's vars are in [`examples/`](examples/).

## Priority

`bd ready` sorts by priority, so priority is the queue order. The finding
template derives it from severity: blocking P1, planned dev P2, important P2,
minor P4. The lead still picks the assignee; the usual case is a frontier dev
for blocking and dev work and a fast agent for important and minor.

---
name: atm-bd-orchestration
version: 0.3.5
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
.claude/skills/atm-beads/scripts/manifest-get bead_prefix   # the repository manifest is readable
```

If anything is missing or too old, **read
[`../atm-beads/references/installation-and-troubleshooting.md`](../atm-beads/references/installation-and-troubleshooting.md)
before proceeding.** The repository facts this skill needs (bead prefix,
trunk, integration branch pattern, governing docs, policy, role map,
commands, stack mechanics) are the manifest `.claude/project/orchestration.yaml`
(`atm-beads` "Manifest"); nothing repo-specific is written in the skill.

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

## Roles

| Role | Who | Prompt |
| --- | --- | --- |
| lead | the appointed lead | this skill |
| dev | frontier / high-value devs; fast agents for important and minor findings | the assignment templates |
| dev-sanity | the member the repository maps to the role, never a dev or fix agent | [`roles/dev-sanity.md`](roles/dev-sanity.md) |
| quality-mgr | the long-running QA agent | [`roles/quality-mgr.md`](roles/quality-mgr.md) |

This skill names roles, not members or agents. A repository maps a role to
its team-unique member in `roles:` of the manifest
`.claude/project/orchestration.yaml` (`roles:` of
`.claude/agents/registry.yaml` is the fallback), and that member's
`[startup.<member>]` prompt in `.atm.toml` names the directive it runs. Resolve the member with
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

Every phase runs as one append-only `gh stack` on the phase integration
branch (manifest `integration_branch`, `integrate/phase-<x>` here). The
mechanics are the documents and skills the manifest names in
`stack.mechanics` (`manifest-get stack.mechanics`), plain markdown any agent
can follow. What this skill changes:

- Dev and fix work runs in parallel. Each task works in its own worktree cut
  from the pushed top at task start, and is not linked while it runs.
- When the work is complete the writer rebases it once onto the current
  pushed top (it is live and has no children, so this is the one allowed
  rebase) and pushes. Layers therefore stack in completion order. When two
  layers finish on the same top, the lead rebases the second onto the first
  before linking it (Loop, sanity check PASS).
- The lead opens the PR (base = the top it was rebased onto) and links it
  when its **sanity check passes**, not at the first push. A sanity check
  failure reopens a branch nothing is stacked on yet, so the dev-fix can
  commit and rebase it again.
- Once linked, a layer is frozen. A later finding on it is fixed on a new
  layer at the top of the same stack, one finding per layer, and fix layers
  land between dev layers.
- The lead records the linked bead's actual `layer` and `pr_target`:
  `bd update <bead> --set-metadata layer=<n> --set-metadata pr_target=<branch>`.
- Landing is one `gh stack merge <stack#> --yes --merge` after the last
  finding closes (the landing procedure in `stack.mechanics`).

## Plan Gate

No dev bead is dispatched until the plan passes review.

1. Validate the plan:
   `.claude/skills/atm-beads/scripts/validate-plan --root <root>`, run from
   the repository root. It runs `bd doctor`, `check-plan.jq`, the REQ/ADR
   existence check and the ATM member check. Exit 0 or stop.
2. Create the plan-review bead right after the import (the import
   procedures do this as their next step), so that it blocks every root sprint (every dev
   bead with no sanity check blocker). For sprints imported into a running
   phase, use `<root>-plan-qa-<n>` (the next free number) and block only the
   new dev beads, including any that already have sanity check blockers:

   ```bash
   bd create --id <root>-plan-qa --type task --parent <root> \
     -l phase-<x>,stage:plan-review --assignee quality-mgr \
     --title "phase-<x>: plan review" --deps blocks:<root sprint>,blocks:<root sprint>
   ```

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
| sanity check PASS | check that the layer's `rebased_onto` (from its dev-complete or fix-complete) is still the pushed top. If another layer was linked since, rebase the branch onto the new top yourself: it is not linked and has no children. Run the test command and push with `--force-with-lease`. On a conflict, `bd reopen` the bead and send a dev-fix naming the new top. Then link the layer, then create the QA bead from [`qa-bead.json.j2`](templates/qa-bead.json.j2) (`validates` the dev or finding bead). For a finding, the QA dispatch sets `checked_bead` = the finding (it carries its own requirements and ADRs), `sprint_bead` = its `metadata.sprint_bead`, `carry_forward` = the finding id, and `round` = the `metadata.round` of the QA bead it was `discovered-from`, + 1, or 1 when it came from the phase-end review |
| sanity check FAIL | `bd reopen <checked bead> --reason "<summary>"`, then [`dev-fix.xml.j2`](templates/dev-fix.xml.j2) with the findings. This applies to a finding bead too: its task id is the finding, and it closes with `dev-complete.md.j2` |
| qa-complete | nothing to wire: quality-mgr filed and wired the finding beads; they are in the next `bd ready` |
| fix-complete (`fixed`) | create the fix's sanity check bead (`atm-beads` [`dev-sanity-bead.json.j2`](../atm-beads/templates/dev-sanity-bead.json.j2), `dev_bead` = the finding) |
| review-complete | file each finding with `finding-bead.json.j2` (`qa_bead` = the review bead) |
| task-refused | read the reason and the bead state (`open`, or `blocked-failed` for a dev bead that declared failure). Reassign it, split it, or close the bead yourself with `bd close <bead> --force --reason "<why>"`. A `blocked` bead is never in `bd ready`: run `bd update <bead> --status open --assignee <new agent>` before you re-dispatch it |
| fix-complete (`not_reproducible`) | nothing: no commit, no sanity check; the finding is closed |
| not-ready report | the task is still open and queued. Fix the cause it names (usually a blocker still open) and tell the assignee to run the ready check again. If the work is no longer wanted, close the task `cancelled` with `task-refused.md.j2` (`bead_state` open) |

Re-run `bd ready` after every close. Never cache the ready list. The open
phase root also appears in it; it is never dispatched.

When every sprint and finding bead is closed
(`bd list -l phase-<x> --status open,in_progress,blocked -n 0 --json` lists only
the phase root), run the phase-end review (below). When its findings are
closed too, land the stack (the landing procedure in `stack.mechanics`),
close the phase root, and run `bd sync`.

### Phase-End Review

Create the review bead, then dispatch it with `review-template.xml.j2`:

```bash
bd create --id <root>-review --type task --parent <root> \
  -l phase-<x>,stage:review --assignee <reviewer> \
  --title "phase-<x>: phase-end review"
```

On review-complete, file each finding with `finding-bead.json.j2`, using:

- `qa_bead` = the review bead;
- `caused_by` = the dev bead whose code it cites, or the phase root when it
  spans sprints;
- `sprint` and `found_on_layer` from that bead's metadata (`phase-end` and
  the top layer when it is the root);
- `found_at_commit` = the reviewed commit;
- `screen` = `keep`, unless you ran `ceremony-finding-screen` over them;
- `finding_ref` = the reviewer's numbering (`R-1`, `R-2`, …);
- `sprint_bead`, `requirements` and `adrs` = the cited dev bead's id and
  lists. For a finding that spans sprints, use the root as `sprint_bead`. For
  `requirements` and for `adrs` separately, take the union of the real ids
  of the sprints it touches, dropping every `NONE` and duplicate. Use
  exactly `["NONE"]` only when that union is empty.

Fix them as for any finding.

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

- Before the first dispatch of a phase, create the integration branch
  (`manifest-get integration_branch`, `{phase}` = the phase id) from the
  trunk (`manifest-get trunk`) and push it.
- For a dev or finding bead, create its branch and worktree from the pushed
  top: `git fetch origin && git worktree add -b <branch> <worktree>
  origin/<top>`. That is `top` in the vars, and `integrate/phase-<x>` while
  the stack is empty.
- Set the bead's assignee to the recipient first:
  `bd update <bead> --assignee <agent>`. For a role, the recipient is
  `resolve-role <role>` (a sanity check bead is already assigned to it). A claim fails when the bead is
  assigned to anyone else.
- Build vars from the template's `required_variables`, with `task_id` = the
  bead id; the bead supplies most of the rest
  (`bd show <bead> --json | jq '.[0].metadata'`). Templates cannot run
  scripts, so their defaults (`lint_command`, `test_command`, `policy_path`,
  `trunk`) are only defaults: take the values from the manifest and put
  them in every vars file, for example
  `M=.claude/skills/atm-beads/scripts/manifest-get; jq -n --arg lint "$($M commands.lint)" --arg test "$($M commands.test)" --arg policy "$($M policy)" --arg trunk "$($M trunk)" '{lint_command: $lint, test_command: $test, policy_path: $policy, trunk: $trunk}'`
  merged with the bead's values. Keep vars files outside the repository.
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
| `BEADS_ACTOR` empty falls back to git `user.name` | `--actor "$ATM_IDENTITY"` on every `bd` write | the actor is always `ATM_IDENTITY` |
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
template derives it from severity: blocking P1, planned dev P2, important P3,
minor P4. The lead still picks the assignee; the usual case is a frontier dev
for blocking and dev work and a fast agent for important and minor.

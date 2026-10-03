---
name: atm-beads
version: 0.3.0
description: Plans written as beads. Use when writing, validating or importing a phase plan into beads, or when pairing an ATM task with its bead (claim, start, close).
requires:
  cli:
    - name: bd
      minimum_version: 1.3.0
    - name: atm
    - name: sc-compose
    - name: jq
depends_on:
  atm-bd-orchestration: 0.x
---

# ATM Beads

Beads are the plan and the work graph; ATM tasks are the dispatch and the
span. There are no plan markdown files: the phase is an epic and each sprint
is a dev bead followed by a sanity check bead. A bead id is the ATM
`task_id`, and the two open and close together.

## Step 1 — Verify CLI Installation

Run this before anything else in the skill:

```bash
for c in bd atm sc-compose jq; do command -v "$c" >/dev/null && echo "ok $c" || echo "MISSING $c"; done
bd version    # 1.3.0 or newer
```

If anything is missing or too old, **read
[`references/installation-and-troubleshooting.md`](references/installation-and-troubleshooting.md)
before proceeding.**

## Identity

- `ATM_IDENTITY` and `BEADS_ACTOR` are already in every agent's environment
  and are equal: the bare pane name (`team-lead`), never an alias (`atm-lead`) and
  never a model class (`terra`).
- A bead's assignee is the recipient's `ATM_IDENTITY`.

## Lifecycle

Every assignment is one ATM task and one bead, opened and closed together,
and the task id is the bead id: `bd update <bead> --claim` then
`atm task start`, and `bd close <bead>` with
`atm task close <bead> completed --template <complete> --vars <file>`. The
templates and the not-ready rule are in the `atm-bd-orchestration` skill. A
push or progress report closes neither.

## Resources

Read only the one the current job needs.

| Resource | Read when |
| --- | --- |
| [`resources/planning.md`](resources/planning.md) | writing or reviewing the plan: the phase root, dev and sanity check beads, their fields, metadata and stack order |
| [`resources/atm-beads-plan-guidelines.md`](resources/atm-beads-plan-guidelines.md) | shaping the sprints themselves: boundaries, closure, tracks, waves, naming (read "sprint doc" as "sprint bead") |
| [`resources/orchestrating.md`](resources/orchestrating.md) | lead work: checking the plan beads, wiring QA and fix dependencies, dispatching from `bd ready` (templates: the `atm-bd-orchestration` skill) |
| [`resources/importing-md-plan.md`](resources/importing-md-plan.md) | importing an existing markdown plan into beads: one sprint doc, or a whole phase; includes the missing-info checks |
| [`resources/dev-sanity.md`](resources/dev-sanity.md) | writing or sending the sanity check assignment (recipient and message) |
| [`resources/troubleshooting.md`](resources/troubleshooting.md) | a claim, close or assignee looks wrong, or `bd ready` misses assigned work |

Every phase plan must include a committed `docs/plans/phase-<x>/sprints.jsonl`.
Each line is `[sprint_name, sanity_bead_id, depends_on_sprint_names]`; the dev
bead ID is derived as `atm-<sprint_name>`. The planner writes this compact
graph authority in the plan PR; it is never exported from mutable Beads state.
Bead hierarchy: `resources/planning.md`.
The initial `docs/plans/phase-<x>/phase-<x>-dag.html` (embedded SVG) must also be
committed and pushed with the plan on the phase integration branch before
plan review. `sprint-review --root <root>` refreshes the HTML without a viewer;
`--view` optionally opens Wyvern in the background.

## Validation

Validation is mandatory before a plan is imported, before plan review and
before the first dispatch. Run it from the repository root:

```bash
.claude/skills/atm-beads/scripts/validate-plan --file <plan.jsonl> --root <root id> --index <sprints.jsonl>   # before import
.claude/skills/atm-beads/scripts/validate-plan --root <root id>   # live beads, plan from origin/develop
```

It checks four things:

1. every sprint that `sprints.jsonl` says depends on sprint X has a `blocks`
   edge to X's sanity bead;
2. every sprint bead validates against the `SprintBead` model: `requirements`
   and `adrs` (ids, or exactly `["NONE"]`), `worktree`, `branch`,
   `pr_target`, an assignee, a numbered `## Deliverables` list, acceptance
   criteria and a valid difficulty;
3. every sanity bead validates against the `SanityBead` model (an assignee,
   `metadata.dev_bead`, a `blocks` edge to it), and `dev_bead` is its sprint;
4. `bd doctor` reports no error.

The models are pydantic, in `scripts/bead_schema.py`; `schemas/*.schema.json`
are exported from them (`bead_schema.py export schemas`) and published.
Exit 0 means valid, 5 lists the problems, and 2 means it could not run.
Report problems to the lead; never edit the script, the plan or the graph to
make it pass.

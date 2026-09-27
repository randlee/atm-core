---
name: atm-beads
version: 0.1.0
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
  and are equal: the bare pane name (`team-lead`), never an alias (`atm-dev-lead`) and
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

Every phase plan must include a committed `docs/plans/phase-<x>/sprints.json`
listing only the phase root ID and paired dev/sanity bead IDs that define the phase’s work; creating and updating
this list is a mandatory planning step before plan review or dispatch. Beads
are the source of truth; bead content is not duplicated in the index.
The initial `docs/plans/phase-<x>/phase-<x>-dag.html` (embedded SVG) must also be
committed and pushed with the index on the phase integration branch before
plan review. `sprint-review --root <root>` produces both without a viewer;
`--view` optionally opens Wyvern in the background.

## Validation

Validation is mandatory before a plan is imported, before plan review and
before the first dispatch. Run it from the repository root:

```bash
.claude/skills/atm-beads/scripts/validate-plan --file <plan.jsonl>   # rendered, before import
.claude/skills/atm-beads/scripts/validate-plan --root <root id>      # live beads
```

It runs `bd doctor` first. It fails on any doctor error, a missing field,
a broken graph, a missing, empty or unknown REQ/ADR id (`["NONE"]` is the
only way to say there is none), or an assignee who is not an ATM member.
Exit 0 means valid, 5 lists the problems, and 2 means it could not run.

---
name: atm-beads
version: 0.3.5
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
- A bead's assignee is the recipient's `ATM_IDENTITY`, set at dispatch; a planned bead carries only `difficulty`.

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

Every phase plan must include the plan file `<plans_dir>/phase-<x>.jsonl` and the
tracked phase file `.atm-bd/phase-<x>.toml` (format: `resources/planning.md`
"Phase definition"), committed and pushed on the phase root's `integration_branch`
before plan review. `.claude/skills/sprint-review/scripts/sprint-review --root <root>` writes
`<plans_dir>/phase-<x>/phase-<x>-dag.html` locally (never commits or pushes) without a
viewer; `--view` optionally opens Wyvern in the background. `plans_dir` and
the other repository values come from the repository configuration
(`atm-bd-orchestration` SKILL.md, "Repository configuration").

## Validation

Validation is mandatory before a plan is imported, before plan review and
before the first dispatch. Run it from the repository root:

```bash
.claude/skills/atm-beads/scripts/validate-plan --file <plan.jsonl> --phase <x> --index <plans_dir>/phase-<x>.jsonl   # before import
.claude/skills/atm-beads/scripts/validate-plan --phase <x>   # live beads; plan file from origin/<integration_branch>
.claude/skills/atm-beads/scripts/validate-plan --ci   # CI, offline: every tracked .atm-bd/phase-*.toml and its plan file parse
```

What it checks is listed once, in the header of
[`.claude/skills/atm-beads/scripts/validate-plan`](scripts/validate-plan). The bead models are pydantic,
in `.claude/skills/atm-beads/scripts/bead_schema.py`; `schemas/*.schema.json` are exported from them
(`python3 .claude/skills/atm-beads/scripts/bead_schema.py export schemas`) and published. Exit 0 means valid, 5 lists
the problems, and 2 means it could not run (the reason is on stderr, including
`bd doctor`'s own stderr). Report problems to the lead; never edit the script,
the plan or the graph to make it pass.

---
name: atm-beads
version: 0.3.1
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
.claude/skills/atm-beads/scripts/manifest-get bead_prefix   # the repository manifest is readable
```

If anything is missing or too old, **read
[`references/installation-and-troubleshooting.md`](references/installation-and-troubleshooting.md)
before proceeding.**

## Manifest

Every repository-specific fact the skill needs is in one file,
`.claude/project/orchestration.yaml`, read with
`.claude/skills/atm-beads/scripts/manifest-get <dotted.key> [--json]`
(exit 2 when the file or the key is missing):

| Key | Meaning | atm-core |
| --- | --- | --- |
| `bead_prefix` | prefix of every bead id (`<prefix>-phase-<x>`, `<prefix>-<x>-<n>`) | `atm` |
| `trunk` | the branch phases are cut from and merge back to | `develop` |
| `integration_branch` | the phase branch pattern, `{phase}` = the phase id | `integrate/phase-{phase}` |
| `docs.requirements`, `docs.architecture` | globs of the governing documents; an id counts as known when it is in a file's content or its file name | `docs/requirements.md`; `docs/architecture.md`, `docs/adr/ADR-*.md` |
| `policy` | the repository QA policy | `.claude/project/quality-policy.md` |
| `roles` | role → team-unique ATM member (`resolve-role`) | `dev-sanity: atm-sanity`, `quality-mgr`, `lead: team-lead` |
| `commands.lint`, `commands.test`, `commands.validate` | the sanity-check lint, the dev/fix test run, the phase-end validation | `just lint`, `just test`, `just validate` |
| `stack.mechanics` | the stack documents and skills, in reading order | `docs/development/gh-stack-guidelines.md`, `gh-stack-view` |
| `sanity.child_model` | the check child model of a Codex dev-sanity member | see the file |

The scripts read it themselves; the lead copies its values into template
vars (`atm-bd-orchestration` "Dispatch"). Prose in these skills writes ids
as `<prefix>-…`.

## Identity

- `ATM_IDENTITY` and `BEADS_ACTOR` are already in every agent's environment
  and are equal: the bare pane name (`fenix`), never an alias (`atm-lead`) and
  never a model class (`fable`).
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

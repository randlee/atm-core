# Planning in Beads

The plan is written to beads, not to markdown files. The phase is a root
bead (an epic, or a feature under the Development epic). Every sprint is a
sprint container under it; `bead-groups` pours its dev, sanity and qa beads
under the container. The dependency edges are the order. There is no phase plan or sprint doc to keep
in sync: `bd show <bead>` is the plan.

Shape the sprints with
[`atm-beads-plan-guidelines.md`](atm-beads-plan-guidelines.md). Where it says
"sprint doc", write the same content into the sprint bead as below. The
stack rules below override the guidelines' track stacks.

To turn an existing markdown plan into beads, use
[`importing-md-plan.md`](importing-md-plan.md) instead.

## Writing The Plan

Every bead is rendered from a template, so a missing field fails the render
rather than reaching an agent:

| Bead | Template | Id |
| --- | --- | --- |
| phase root | [`plan-root.json.j2`](../templates/plan-root.json.j2) | `<prefix>-phase-<x>` |
| sprint container | [`sprint-bead.json.j2`](../templates/sprint-bead.json.j2) | `<prefix>-<x>-<n>` |

1. Run `bd doctor --json`. Any check with `"status": "error"` stops the
   plan; report it to lead.
2. Write one vars file per bead (examples in [`../examples/`](../examples/))
   and render each strictly into one JSONL file:

   ```bash
   sc-compose render --file .claude/skills/atm-beads/templates/<t>.json.j2 \
     --var-file <scratch>/<bead>-vars.json --strict --output <scratch>/<bead>.json \
     && jq -e -c . <scratch>/<bead>.json >> <scratch>/plan.jsonl
   ```

   Stop on any failure; never pipe a render straight into `jq`, which drops
   a failed bead silently.

3. Write the plan file `<plans_dir>/phase-<x>.jsonl` and the phase file
   `.atm-bd/phase-<x>.toml` by hand (see "Phase definition" below).
4. Validate the rendered plan against it. This step is mandatory:

   ```bash
   .claude/skills/atm-beads/scripts/validate-plan --file <scratch>/plan.jsonl \
     --phase <x> --index <plans_dir>/phase-<x>.jsonl
   ```

   Exit 5 lists every problem. Fix them all and render again.
5. `bd import --dry-run -i <scratch>/plan.jsonl`, then `bd import -i
   <scratch>/plan.jsonl`. Pour every sprint's group:
   `.claude/skills/atm-bd-orchestration/scripts/bead-groups --phase <x>`.
   Right away, create the plan-review bead
   (`atm-bd-orchestration` "Plan Gate", step 1). Commit the plan file and the
   phase file and push them to the root bead's `integration_branch`. Then run
   `.claude/skills/sprint-review/scripts/sprint-review --root <root>`, which
   writes `<plans_dir>/phase-<x>/phase-<x>-dag.html` locally; it never commits
   or pushes. No viewer opens without `--view`. Then run `validate-plan --phase <x>`
   on the imported beads; without `--index` it reads the plan file from
   that integration branch.

The plan then goes to plan review (`atm-bd-orchestration` "Plan Gate").
Nothing is dispatched until it passes.

Keep `<scratch>` outside the repository.

## Phase definition (plan file and phase file)

The plan file `<plans_dir>/phase-<x>.jsonl` is the authored, committed sprint
set: one line per sprint, the sprint container id is `<prefix>-<sprint>`.
Sprint content (title, deliverables, acceptance, REQ/ADR, ownership, and
state) lives only in Beads.

```jsonl
{"sprint": "d-12"}
{"sprint": "d-13", "depends_on": ["d-12"]}
```

`depends_on` names hard prerequisite sprints only; `bead-groups` adds the
`blocks` edge from the dependent's dev bead (`<container>.group-dev`) to the
predecessor's sanity bead (`<pred>.group-sanity`). `<prefix>` is taken from the
phase root id (`<prefix>-phase-<x>`). No finding, fix, QA, task, branch, or
runtime gate appears in this file.

The tracked phase file `.atm-bd/phase-<x>.toml` holds exactly:

```toml
plan = "<plans_dir>/phase-<x>.jsonl"
root = "<prefix>-phase-<x>"
integration_branch = "integrate/phase-<x>"
```

The file is never generated from beads and beads are never generated from the
file. A plan change is one planner transaction: change the beads, edit the
file, commit both. `validate-plan --phase <x>` checks the sprint containers
and poured beads against it (see `SKILL.md`, Validation). It must stay green from
plan approval to phase end; the dev and fix templates run it before a claim.

Hierarchy:

- top level: epics only; the phase root is an epic or a `feature` under epics;
- children of the root: exactly the listed sprint containers, plus `stage:plan*` beads,
  `bd gate` beads and sprints closed "folded into ...";
- under the sprint container: its poured `dev ← sanity ← qa` group and one
  poured `fix ← sanity ← qa` group per blocking finding. Important and minor
  findings are plain finding beads under the phase or feature bead.

`phase-<x>-dag.html` is written locally; it is never committed or pushed.
`--view` only controls optional background viewing in Wyvern.

## Phase Root

| Var | Content |
| --- | --- |
| `title` | the phase title; the bead title becomes `phase-<x>: <title>` |
| `description` | goal and the sprint table |
| `design` | phase-level architecture decisions, the boundaries in scope, retained gates |
| `acceptance_criteria` | the phase-level gates |
| `plan_scope`, `parent` | `feature` under the Development epic, or `epic` with no parent |
| `integration_branch` | required: `integration_branch_pattern` from the repository configuration with `{phase}` = `<x>`; validate-plan, sprint-review and every gate read the plan from this branch |

## Sprint Container

| Var | Content |
| --- | --- |
| `title` | the sprint title; the bead title becomes `<sprint>: <title>` |
| `description` | goal, deliverables, required work, and what the sprint does not close |
| `design` | public contract, types, code samples, exact targets |
| `acceptance_criteria` | acceptance criteria and the validation commands |
| `parent` | the phase root |

Its labels (`phase-<x>`, `stage:sprint`, `stack:<stack>`, `train:<t>` when
set) and metadata come from these required vars:

| Var | Value |
| --- | --- |
| `sprint` | `d-5` |
| `stack` | `phase-<x>`: the phase is one stack |
| `layer` | planned position, 1 = bottom |
| `branch` | `sprint/<phase>-<n>-<slug>` |
| `pr_target` | planned: branch of its nearest `must_follow` prerequisite, the one it builds on, or the root's `integration_branch` when it has none; never a parallel sibling; a lower bound: the PR's actual base is it or a descendant of it |
| `worktree` | `<worktree_base>/<branch>` |
| `relation` | `root`, `must_follow` or `parallel_safe` |
| `closure_type` | from the guidelines' closure types |
| `target_boundary` | the one boundary the sprint closes |
| `owned_paths` | files and crates the sprint owns: its file fence |
| `requirements` | every REQ id that governs the work (`LOG-001`, `OTLP-008`, `ATM-BASE-3`, `NFR-…`), or exactly `["NONE"]` |
| `adrs` | every ADR that governs the work (`ADR-011`), or exactly `["NONE"]` |
| `difficulty` | `hard`, `normal` or `fast`; the plan names no agent |

Optional: `model_class` (`astra`, `terra`, `luna`), `release_train`,
`priority`.

`requirements` and `adrs` are never left empty. The dev reads each listed id
before coding, QA checks the change against each one, and plan review
rejects a list that is missing, names an id that does not exist or does not
govern the work, or leaves out one the work touches. `NONE` is a claim the
author makes, and plan review checks it like any other.

### New Ids

This is the one exception to "the id must exist", and every other file
links here. A sprint may list a requirement or ADR id that its governing
document does not have yet only when both of these hold:

1. the document is in the sprint's `owned_paths`;
2. a deliverable in the bead's description names the id and says the sprint
   adds it.

Plan review checks both (`validate-plan` does not); if either fails, the id
is unknown and blocking. QA
checks that the definition landed in the document at the reviewed commit;
if it did not, that is a blocking finding.

Branch and id naming follows the repository's "Plan Naming" in its QA policy
(`policy_path` in the repository configuration) and the shared rules in the guidelines'
"Naming" section; the doc file names there do not apply.

## Dev Sanity Check Bead

Not planned: `bead-groups` pours it as `<container>.group-sanity`
(`dev_bead` = the group's dev bead, no assignee). It is blocked by its dev bead, and later sprints
wait on it rather than on the dev bead, so a sprint's dependents start only
after its work passes the sanity check. See [`dev-sanity.md`](dev-sanity.md).

## Stack

- The phase is one append-only `gh stack` on the root's `integration_branch`.
  Only the final phase PR leaves it, for the repository's base branch.
- `layer` is the plan's intent. Layers really stack in the
  order they complete, and the lead records the actual `layer` when it links
  each one (`atm-bd-orchestration` "Stack Discipline"); `pr_target` stays the planned lower bound until a dev-fix records the sprint's first layer's branch there.
- Sprints that can run at once must have disjoint `owned_paths`. Sprints that
  share a path must be ordered: one's sanity bead in the other's blocker closure.

The stack table is a query, not a document:

```bash
bd list -l phase-d,stage:sprint -n 0 --json | jq -r '.[] | [.metadata.stack, .metadata.layer, .metadata.sprint, .metadata.branch, .metadata.pr_target, .assignee] | @tsv' | sort
```

## Checks

`validate-plan` is the scripted check; the plan is not ready for plan review
until it exits 0. What it checks is listed once, in the header of
[`scripts/validate-plan`](../scripts/validate-plan), and the field rules are
the models in [`scripts/bead_schema.py`](../scripts/bead_schema.py). Everything
else in this file (governing ids exist and govern the work, `owned_paths`
of concurrent sprints are disjoint, `relation`, `layer` and `pr_target` agree)
is checked by plan review, not by the script.

# Planning in Beads

The plan is written to beads, not to markdown files. The phase is a root
bead (an epic, or a feature under the Development epic). Every sprint is two
beads under it: a dev bead, and the sanity check bead that follows it. The
dependency edges are the order. There is no phase plan or sprint doc to keep
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
| sprint dev bead | [`sprint-bead.json.j2`](../templates/sprint-bead.json.j2) | `<prefix>-<x>-<n>` |
| sprint sanity check | [`dev-sanity-bead.json.j2`](../templates/dev-sanity-bead.json.j2) | `<dev id>-sanity` |

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

3. Validate. This step is mandatory:

   ```bash
   .claude/skills/atm-beads/scripts/validate-plan --file <scratch>/plan.jsonl
   ```

   Add `--root <id> --phase <x>` when the root already exists. Exit 5 lists
   every problem. Fix them all and render again.
4. `bd import --dry-run -i <scratch>/plan.jsonl`, then `bd import -i
   <scratch>/plan.jsonl`. Right away, create the plan-review bead
   (`atm-bd-orchestration` "Plan Gate", step 2). Then run
   `validate-plan --root <root>` on the imported beads, and `bd sync`.

The plan then goes to plan review (`atm-bd-orchestration` "Plan Gate").
Nothing is dispatched until it passes.

Keep `<scratch>` outside the repository.

## Phase Root

| Var | Content |
| --- | --- |
| `title` | the phase title; the bead title becomes `phase-<x>: <title>` |
| `description` | goal and the sprint table |
| `design` | phase-level architecture decisions, the boundaries in scope, retained gates |
| `acceptance_criteria` | the phase-level gates |
| `plan_scope`, `parent` | `feature` under the Development epic, or `epic` with no parent |
| `integration_branch` | `integrate/phase-<x>` |

## Sprint Dev Bead

| Var | Content |
| --- | --- |
| `title` | the sprint title; the bead title becomes `<sprint>: <title>` |
| `description` | goal, deliverables, required work, and what the sprint does not close |
| `design` | public contract, types, code samples, exact targets |
| `acceptance_criteria` | acceptance criteria and the validation commands |
| `assignee` | the ATM identity that owns it (`aobs`); must be in `atm members` |
| `parent` | the phase root |
| `blocked_by` | the **sanity check** bead of each prerequisite sprint (`atm-bd-4-sanity`), never its dev bead |

Its labels (`phase-<x>`, `stage:dev`, `stack:<stack>`, `train:<t>` when
set) and metadata come from these required vars:

| Var | Value |
| --- | --- |
| `sprint` | `d-5` |
| `stack` | `phase-<x>`: the phase is one stack |
| `layer` | planned position, 1 = bottom |
| `branch` | `sprint/<phase>-<n>-<slug>` |
| `pr_target` | planned: branch of layer n−1, or `integrate/phase-<x>` for layer 1 |
| `worktree` | `<repo>-worktrees/<branch>` |
| `relation` | `root`, `must_follow` or `parallel_safe` |
| `closure_type` | from the guidelines' closure types |
| `target_boundary` | the one boundary the sprint closes |
| `owned_paths` | files and crates the sprint owns: its file fence |
| `requirements` | every REQ id that governs the work (`REQ-ATM-CMD-001`, `REQ-CORE-BOUNDARY-001`, `NFR-…`), or exactly `["NONE"]` |
| `adrs` | every ADR that governs the work (`ADR-011`), or exactly `["NONE"]` |

Optional: `model_class` (a model class name the team uses), `release_train`,
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

`validate-plan` checks the first and reports the id as a warning. Plan
review checks the second; if it fails, the id is unknown and blocking. QA
checks that the definition landed in the document at the reviewed commit;
if it did not, that is a blocking finding.

Branch and id naming follows the repository's "Plan Naming" in
`.claude/project/quality-policy.md` and the shared rules in the guidelines'
"Naming" section; the doc file names there do not apply.

## Dev Sanity Check Bead

One per sprint: `dev_bead` = the sprint's dev bead, `assignee` = the
member of the `dev-sanity` role (`scripts/resolve-role dev-sanity`). It is blocked by its dev bead, and later sprints
wait on it rather than on the dev bead, so a sprint's dependents start only
after its work passes the sanity check. See [`dev-sanity.md`](dev-sanity.md).

## Stack

- The phase is one append-only `gh stack` on `integrate/phase-<x>`. Only the
  final phase PR targets `develop`.
- `layer` and `pr_target` are the plan's intent. Layers really stack in the
  order they complete, and the lead records the actual values when it links
  each one (`atm-bd-orchestration` "Stack Discipline").
- Sprints that can run at once must have disjoint `owned_paths`. Sprints that
  must share a file are ordered with `must_follow`.

The stack table is a query, not a document:

```bash
bd list -l phase-bd,stage:dev -n 0 --json | jq -r '.[] | [.metadata.stack, .metadata.layer, .metadata.sprint, .metadata.branch, .metadata.pr_target, .assignee] | @tsv' | sort
```

## Checks

`validate-plan` is the check. The plan is not ready for plan review until it
exits 0. It fails when:

- `bd doctor` reports an error;
- a bead lacks a required field, label or metadata key;
- `requirements` or `adrs` is empty, mixes `NONE` with ids, holds a
  malformed id, or names an id that its governing document does not have;
- a dev bead does not have exactly one sanity check bead, or is blocked by a
  bead other than a sanity check (or the plan-review bead);
- a `root` sprint has prerequisites, or a `must_follow` sprint has none;
- two beads claim the same `stack` and `layer`, or a layer's `pr_target` is
  not the branch of the layer below;
- a bead's parent is not the phase root, or its phase does not match;
- an assignee is not an ATM member of the team, or a sanity check bead's
  assignee is not the `dev-sanity` role's member.
